//! Glyph outline rasterization.
//!
//! This replaces `fontdue`, which reached `ttf-parser` (RUSTSEC-2026-0192,
//! unmaintained) and was the last path to it once the shaper moved to
//! `harfrust`. The replacement is `skrifa` for outlines and `tiny-skia` —
//! already a dependency for the rest of the raster backend — for coverage.
//!
//! # The bounds convention, which is the whole subtlety here
//!
//! A rasterized glyph is an alpha bitmap plus enough metrics to place it. Those
//! metrics are reported relative to the pen, and getting the vertical one wrong
//! shifts every glyph down by its own height while still producing a plausible
//! image: the page fills, the text is present, the ink count barely moves.
//!
//! So, precisely, and matching what `draw_text` assumes:
//!
//! * `xmin` — the glyph's left edge in px from the pen, **fractional**.
//! * `ymin` — the glyph's **lowest** point in px from the baseline, y-up. This
//!   is negative for any glyph that descends.
//! * `height` — the glyph's vertical extent in px, fractional.
//!
//! Placement then works out as `baseline - ymin - height` for the top edge, and
//! the caller rounds to whole pixels. Reporting the *top* edge as `ymin` instead
//! of the bottom puts every glyph exactly one `height` too low.
//!
//! The bitmap itself is an integer-sized box, but the glyph keeps its
//! fractional offset *inside* it, so antialiasing still sees the sub-pixel
//! position. Snapping the glyph to whole pixels inside the bitmap would throw
//! that away and quantise every stem.

use skrifa::{
    instance::{LocationRef, Size},
    outline::{DrawSettings, OutlinePen},
    GlyphId, MetadataProvider,
};
use tiny_skia::{FillRule, Mask, PathBuilder, Transform};

/// A rasterized glyph: an alpha mask and the metrics needed to place it.
#[derive(Debug, Clone)]
pub struct Glyph {
    /// Coverage mask width in px.
    pub w: usize,
    /// Coverage mask height in px.
    pub h: usize,
    /// Left edge in px from the pen, fractional. See the module docs.
    pub xmin: f32,
    /// Lowest point in px from the baseline, y-up, fractional. See the module
    /// docs; this is *not* the top edge.
    pub ymin: f32,
    /// Vertical extent in px, fractional.
    pub height: f32,
    /// Horizontal advance in px, from the font's own metrics.
    pub advance: f32,
    /// Row-major coverage, `w * h` bytes, `0` = transparent.
    pub bitmap: Vec<u8>,
}

/// An `OutlinePen` that builds a `tiny-skia` path and records its bounds.
///
/// Bounds are tracked on control points only. That is not the true outline
/// extent — a curve can bulge past its control points — but the difference is
/// well under a pixel and it keeps the pen allocation-free and branch-light on
/// a path that runs per glyph. The bitmap box is derived from the same numbers,
/// so glyph and box always agree.
#[derive(Default)]
struct PathPen {
    path: PathBuilder,
    min_x: f32,
    max_x: f32,
    min_y: f32,
    max_y: f32,
    started: bool,
}

impl PathPen {
    fn new() -> Self {
        Self {
            min_x: f32::MAX,
            max_x: f32::MIN,
            min_y: f32::MAX,
            max_y: f32::MIN,
            ..Self::default()
        }
    }

    fn note(&mut self, x: f32, y: f32) {
        self.min_x = self.min_x.min(x);
        self.max_x = self.max_x.max(x);
        self.min_y = self.min_y.min(y);
        self.max_y = self.max_y.max(y);
    }
}

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.started = true;
        self.note(x, y);
        self.path.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.note(x, y);
        self.path.line_to(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.note(cx0, cy0);
        self.note(x, y);
        self.path.quad_to(cx0, cy0, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.note(cx0, cy0);
        self.note(cx1, cy1);
        self.note(x, y);
        self.path.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }

    fn close(&mut self) {
        self.path.close();
    }
}

/// Rasterize one glyph at `px`.
///
/// `location` is the variation axis position; the default instance is used when
/// no variation axes are set, which is every bundled font here.
pub fn rasterize(
    font: &skrifa::FontRef<'_>,
    gid: u16,
    px: f32,
    location: LocationRef<'_>,
) -> Option<Glyph> {
    let glyph_id = GlyphId::new(gid as u32);
    let size = Size::new(px);

    // The advance comes from the font's metrics rather than from the outline, so
    // a glyph with no contours still reports the advance the layout measured
    // with. A layout and paint disagreement here would show up as text measured
    // at one width and drawn at another.
    let advance = font.glyph_metrics(size, location).advance_width(glyph_id)?;

    let settings = DrawSettings::unhinted(size, location);
    let mut pen = PathPen::new();
    let outline = font.outline_glyphs().get(glyph_id)?;
    let _ = outline.draw(settings, &mut pen);

    if !pen.started {
        // No contours: space, or a control glyph. Still advance the pen.
        return Some(Glyph {
            w: 0,
            h: 0,
            xmin: 0.0,
            ymin: 0.0,
            height: 0.0,
            advance,
            bitmap: Vec::new(),
        });
    }

    // The bitmap is an integer box, but the glyph keeps its fractional offset
    // within it, so the sub-pixel position survives into the antialiasing.
    let x0 = pen.min_x.floor();
    let y0 = pen.min_y.floor();
    let x1 = pen.max_x.ceil();
    let y1 = pen.max_y.ceil();
    let w = (x1 - x0) as u32;
    let h = (y1 - y0) as u32;
    if w == 0 || h == 0 {
        return None;
    }

    // Move the glyph into the bitmap and flip y: font outlines are y-up with
    // the origin on the baseline, masks are y-down from the top.
    let path = pen.path.finish()?;
    let mut mask = Mask::new(w, h)?;
    mask.fill_path(
        &path,
        // `glyf` uses the non-zero winding rule; CFF charstrings are defined
        // the same way. A font with genuinely overlapping contours in the same
        // direction is a malformed font, and the wrong rule would fill the
        // overlap rather than leaving it knocked out.
        FillRule::Winding,
        true,
        Transform::from_row(1.0, 0.0, 0.0, -1.0, -x0, y1),
    );

    Some(Glyph {
        w: w as usize,
        h: h as usize,
        xmin: pen.min_x,
        ymin: pen.min_y,
        height: pen.max_y - pen.min_y,
        advance,
        bitmap: mask.data().to_vec(),
    })
}

/// Does this font map `ch` to a glyph?
///
/// Used by the missing-glyph path to decide whether the system fallback can
/// help. This is a cmap question, not an outline question, so it is answered
/// without touching glyph outlines at all.
#[must_use]
pub fn has_glyph(font: &skrifa::FontRef<'_>, ch: char) -> bool {
    font.charmap().map(ch).is_some_and(|g| g != GlyphId::NOTDEF)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn embedded() -> &'static [u8] {
        hikari_core::builtin_bytes()
    }

    #[test]
    fn metrics_are_reported_relative_to_the_pen_and_baseline() {
        // The convention in the module docs, pinned. Each assertion here has a
        // wrong value that still renders *something*: reporting the top edge as
        // `ymin` instead of the bottom shifts every glyph down by its own
        // height, and the page still fills.
        let font = skrifa::FontRef::new(embedded()).expect("font");
        let loc = LocationRef::default();

        // 'H' sits on the baseline: min y is 0 and the extent is the cap height.
        let gid = font.charmap().map('H').expect("H is in the font").to_u32() as u16;
        let g = rasterize(&font, gid, 100.0, loc).expect("H rasterizes");
        assert!(
            g.ymin.abs() < 0.5,
            "'H' must sit on the baseline, got ymin {}",
            g.ymin
        );
        assert!(
            (g.height - 72.8).abs() < 2.0,
            "cap height at 100px should be about 72.8, got {}",
            g.height
        );
        assert!(g.xmin > 0.0, "glyph starts to the right of the pen");

        // 'p' descends, so its minimum y is below the baseline.
        let gid = font.charmap().map('p').expect("p").to_u32() as u16;
        let g = rasterize(&font, gid, 100.0, loc).expect("p rasterizes");
        assert!(
            g.ymin < 0.0,
            "'p' descends and must have a negative ymin, got {}",
            g.ymin
        );
    }

    #[test]
    fn glyphs_are_not_pushed_down_by_their_own_height() {
        // The regression this module exists to prevent, stated as an invariant:
        // placement is `baseline - ymin - height`, and for a glyph sitting on
        // the baseline that must land near the baseline, not one height below.
        let font = skrifa::FontRef::new(embedded()).expect("font");
        let loc = LocationRef::default();
        for ch in ['H', 'x', 'o', 'M', 'W'] {
            let gid = font.charmap().map(ch).expect("in font").to_u32() as u16;
            let g = rasterize(&font, gid, 48.0, loc).expect("rasterizes");
            let baseline = 1000.0;
            let top = baseline - g.ymin - g.height;
            let bottom = top + g.height;
            assert!(
                bottom <= baseline + 1.0,
                "{ch:?} descends below the baseline: bottom {bottom} vs baseline \
                 {baseline} (ymin {}, height {})",
                g.ymin,
                g.height
            );
            assert!(
                baseline - top <= g.height + 1.0,
                "{ch:?} top {top} is implausibly far above the baseline"
            );
        }
    }

    #[test]
    fn whitespace_has_an_advance_but_no_pixels() {
        let font = skrifa::FontRef::new(embedded()).expect("font");
        let loc = LocationRef::default();
        let gid = font.charmap().map(' ').expect("space").to_u32() as u16;
        let g = rasterize(&font, gid, 32.0, loc).expect("space rasterizes");
        assert_eq!(g.w, 0);
        assert_eq!(g.h, 0);
        assert!(g.bitmap.is_empty());
        assert!(
            g.advance > 0.0,
            "a space must advance the pen or text would run together"
        );
    }

    #[test]
    fn coverage_is_not_empty_and_not_saturated() {
        // A mask that is all zero draws nothing; all 255 draws a solid block.
        // Both would pass a "did it render" check that only looked for ink, so
        // this asserts the glyph is actually shaded.
        let font = skrifa::FontRef::new(embedded()).expect("font");
        let loc = LocationRef::default();
        let gid = font.charmap().map('o').expect("o").to_u32() as u16;
        let g = rasterize(&font, gid, 64.0, loc).expect("rasterizes");
        assert!(!g.bitmap.is_empty());
        let solid = g.bitmap.iter().filter(|&&v| v == 255).count();
        let empty = g.bitmap.iter().filter(|&&v| v == 0).count();
        let total = g.bitmap.len();
        assert!(
            solid > 0 && solid < total,
            "expected a mix of covered and uncovered pixels, got {solid} solid of {total}"
        );
        assert!(
            empty > 0,
            "a 64px 'o' should have transparent corners, none of {total} pixels were"
        );
        // Antialiased edges mean partial coverage exists; a glyph rasterized with
        // no antialiasing would be entirely 0 or 255.
        let partial = g.bitmap.iter().filter(|&&v| v > 0 && v < 255).count();
        assert!(
            partial > total / 20,
            "expected antialiased edges, only {partial} of {total} pixels are partial"
        );
    }

    #[test]
    fn bitmap_covers_the_reported_bounds() {
        // The bitmap must be wide enough for the glyph, and must not be padded
        // with a lot of empty space: a box much larger than the ink means the
        // bounds and the path disagree.
        let font = skrifa::FontRef::new(embedded()).expect("font");
        let loc = LocationRef::default();
        for ch in ['H', 'o', 'g', 'W', 'i'] {
            let gid = font.charmap().map(ch).expect("in font").to_u32() as u16;
            let g = rasterize(&font, gid, 40.0, loc).expect("rasterizes");
            assert_eq!(
                g.bitmap.len(),
                g.w * g.h,
                "bitmap length must match its box"
            );
            let ink = g.bitmap.iter().filter(|&&v| v > 8).count();
            assert!(ink > 0, "{ch:?} rasterized to no ink at all");
            let fill = ink as f32 / (g.w * g.h) as f32;
            assert!(
                (0.05..0.95).contains(&fill),
                "{ch:?} fills {fill:.2} of its box, which means the box is wrong"
            );
        }
    }

    #[test]
    fn sub_pixel_position_survives_into_the_bitmap() {
        // The glyph keeps its fractional offset inside the bitmap rather than
        // being snapped to whole pixels. Two sizes a fraction apart must
        // therefore produce different coverage, not the same bitmap.
        let font = skrifa::FontRef::new(embedded()).expect("font");
        let loc = LocationRef::default();
        let gid = font.charmap().map('o').expect("o").to_u32() as u16;
        let a = rasterize(&font, gid, 32.0, loc).expect("rasterizes");
        let b = rasterize(&font, gid, 32.05, loc).expect("rasterizes");
        assert_ne!(
            a.bitmap, b.bitmap,
            "a 0.05px size change must alter the raster, otherwise sub-pixel \
             position is being discarded"
        );
        assert!(
            (a.xmin - b.xmin).abs() < 1.0,
            "a 0.05px size change moved the glyph by more than a pixel"
        );
    }

    #[test]
    fn has_glyph_answers_the_cmap_question() {
        let font = skrifa::FontRef::new(embedded()).expect("font");
        assert!(has_glyph(&font, 'A'), "Latin A is covered");
        assert!(has_glyph(&font, '\u{0645}'), "Arabic meem is covered");
        assert!(
            !has_glyph(&font, '\u{4E2D}'),
            "a CJK ideograph is not in DejaVu"
        );
        assert!(
            !has_glyph(&font, '\u{10FFFF}'),
            "an unassigned codepoint is not covered"
        );
    }
}

/// Pixel-level filter application, exposed so it can be tested directly.
pub mod filter {

    /// Apply a colour-only filter to every pixel of `pix`.
    ///
    /// A narrow front door for tests: it skips the blur and opacity passes so a
    /// colour-matrix bug shows up as a colour assertion failure rather than as a
    /// blurred, faded image nobody can read a value out of.
    pub fn apply(pix: &mut tiny_skia::Pixmap, filter: &hikari_core::Filter) {
        super::apply_filter(pix, filter);
    }
}

// ---------------------------------------------------------------------------
// Filter application
// ---------------------------------------------------------------------------

/// Apply `filter` to `pix` in place.
///
/// Two steps, in the order documented on [`hikari_core::Filter`]: a colour matrix
/// folded from whichever colour operations are set, then opacity. Each is skipped
/// when it would be a no-op, so a grayscale-only filter costs one pass over the
/// pixels rather than two.
///
/// Works on a flat copy of the premultiplied RGBA buffer rather than through
/// `Pixmap`'s pixel accessor. Two reasons: the blur needs random access in both
/// axes, which a row-major slice gives naturally; and `PremultipliedColorU8`
/// exposes no setters, so every write would otherwise mean rebuilding and
/// re-storing the colour.
///
/// The buffer is copied once in and once out, which is two extra allocations per
/// filtered node. For a fixed-size render that is the right trade against
/// fighting the accessors, and the alternative -- caching the scratch buffer --
/// would need a thread-local and would buy very little.
///
/// # Blur
///
/// Three box passes, approximating a Gaussian: repeated box blur converges on
/// one, and three passes at a third of the radius lands within about a pixel of
/// a true Gaussian at typical radii. A real Gaussian costs `O(n * r^2)`; a box
/// pass costs `O(n)`, so at a 40px radius this is roughly two orders of magnitude
/// cheaper. The per-pass radius is `sqrt(3 * radius^2 / passes)`, the standard
/// result for three repeated boxes matching a given standard deviation.
pub fn apply_filter(pix: &mut tiny_skia::Pixmap, filter: &hikari_core::Filter) {
    let w = pix.width() as usize;
    let h = pix.height() as usize;
    if w == 0 || h == 0 {
        return;
    }
    let (pixels, _) = pix.data().as_chunks::<4>();
    let mut buf: Vec<[f32; 4]> = pixels
        .iter()
        .map(|c| [c[0] as f32, c[1] as f32, c[2] as f32, c[3] as f32])
        .collect();
    let n = buf.len();

    if let Some(m) = filter.color_matrix() {
        apply_color_matrix(&mut buf, &m);
    }
    if let Some(o) = filter.opacity {
        if (o - 1.0).abs() > f32::EPSILON {
            let o = o.clamp(0.0, 1.0);
            for px in &mut buf {
                for c in px.iter_mut() {
                    *c *= o;
                }
            }
        }
    }

    let out = pix.data_mut();
    for (i, px) in buf.iter().enumerate() {
        for c in 0..4 {
            out[i * 4 + c] = px[c].round().clamp(0.0, 255.0) as u8;
        }
    }
    let _ = n;
}

/// Folded 4x5 affine colour matrix, one pass over the pixels.
///
/// Premultiplied storage is respected: scaling RGB and scaling alpha
/// independently would be correct for straight alpha, but here the stored
/// colour is already multiplied by alpha, so a filter that changed only alpha
/// would leave the colour too dark. Opacity scales all four channels together
/// for the same reason -- see [`hikari_core::Filter`].
fn apply_color_matrix(buf: &mut [[f32; 4]], m: &[[f32; 5]; 4]) {
    for px in buf.iter_mut() {
        let r = px[0];
        let g = px[1];
        let b = px[2];
        for row in 0..3 {
            px[row] = (m[row][0] * r + m[row][1] * g + m[row][2] * b + m[row][4]).clamp(0.0, 255.0);
        }
    }
}

// ---------------------------------------------------------------------------
// Compositing
// ---------------------------------------------------------------------------

/// Composite a premultiplied layer into `dst` at `(ox, oy)`, source-over.
///
/// # Why `Normal`, always
///
/// `PixmapMut::draw_pixmap` accepts no blend mode in tiny-skia 0.11, so a
/// filtered layer cannot be composited through it with one. Hand-rolling all
/// fifteen modes here would mean a *second* implementation of `hue`,
/// `saturation`, `colour` and `luminosity` -- the four non-separable ones, which
/// the per-glyph path in `lib.rs` already gets right -- and a second
/// implementation is precisely how those four would slowly disagree with the
/// first.
///
/// So the layer composites as `Normal`, which is a documented limitation rather
/// than an oversight:
///
/// * `blend` still applies to everything the node *paints* into the layer: its
///   border, background, image and text each blend against their neighbours
///   exactly as they do without a filter.
/// * What does not happen is the filtered layer then blending as a unit against
///   whatever is behind it.
///
/// `filter` is new, so this combination has no prior behaviour to regress, and
/// it is pinned by `filter_blend_composites_as_normal` in the render tests so it
/// cannot change silently.
///
/// # Clipping
///
/// Writes are clipped per pixel rather than per layer, so a layer hanging off the
/// edge of the canvas composites the part that is on it. A fully transparent
/// source pixel is skipped, which is both faster and what keeps a 0/0 division
/// from ever arising.
pub fn composite(dst: &mut tiny_skia::Pixmap, layer: &tiny_skia::Pixmap, ox: i32, oy: i32) {
    let dw = dst.width() as i32;
    let dh = dst.height() as i32;
    let lw = layer.width() as i32;
    let lh = layer.height() as i32;

    // Bounding box of the overlap, so a layer mostly off-canvas does not walk
    // the whole destination.
    let x0 = ox.max(0);
    let y0 = oy.max(0);
    let x1 = (ox + lw).min(dw);
    let y1 = (oy + lh).min(dh);
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    let src = layer.data();
    let out = dst.data_mut();

    for y in y0..y1 {
        let row_src = ((y - oy) * lw) as usize;
        let row_dst = (y * dw) as usize;
        for x in x0..x1 {
            let si = (row_src + (x - ox) as usize) * 4;
            let di = (row_dst + x as usize) * 4;
            let sa = src[si + 3] as u32;
            if sa == 0 {
                continue;
            }
            let da = out[di + 3] as u32;
            // Premultiplied source-over:
            //
            //   out_rgb = src_rgb + dst_rgb * (1 - src_a)
            //   out_a   = src_a   + dst_a   * (1 - src_a)
            //
            // The coverage term is scaled by the *source* alpha, not the
            // destination's. Swapping the two looks correct whenever the
            // destination is transparent -- the case in a unit test with a clear
            // pixmap -- and throws the source away entirely over anything opaque:
            // an inverted card over a white page came out pure white, because
            // `src * (1 - 255)` is zero. A filter is only ever visible against
            // something, so this is exactly the case a "does it render" check
            // cannot catch and only a composited one does.
            let inv = 255 - sa;
            out[di] = (src[si] as u32 + out[di] as u32 * inv / 255).min(255) as u8;
            out[di + 1] = (src[si + 1] as u32 + out[di + 1] as u32 * inv / 255).min(255) as u8;
            out[di + 2] = (src[si + 2] as u32 + out[di + 2] as u32 * inv / 255).min(255) as u8;
            out[di + 3] = (sa + da * inv / 255).min(255) as u8;
        }
    }
}

#[cfg(test)]
mod filter_tests {
    use super::*;
    use hikari_core::Filter;

    /// A pixmap with an opaque white background and an opaque black square.
    fn sample() -> (tiny_skia::Pixmap, u32, u32) {
        let mut p = tiny_skia::Pixmap::new(40, 40).expect("pixmap");
        p.fill(tiny_skia::Color::WHITE);
        // A 20x20 black block at (10,10), written straight into the buffer
        // because `PremultipliedColorU8` has no setters.
        for y in 10..30usize {
            for x in 10..30usize {
                let i = (y * 40 + x) * 4;
                p.data_mut()[i..i + 4].copy_from_slice(&[0, 0, 0, 255]);
            }
        }
        (p, 40, 40)
    }

    #[test]
    fn a_colour_filter_leaves_alpha_alone() {
        let (mut p, _, _) = sample();
        let before: Vec<u8> = p.data()[3..].iter().step_by(4).copied().collect();
        apply_filter(
            &mut p,
            &Filter {
                grayscale: Some(1.0),
                ..Filter::default()
            },
        );
        let after: Vec<u8> = p.data()[3..].iter().step_by(4).copied().collect();
        assert_eq!(before, after, "a colour filter changed alpha");
    }

    #[test]
    fn opacity_scales_premultiplied_channels_together() {
        // Premultiplied storage means colour must scale with alpha, or a faded
        // red becomes a faded grey.
        let mut p = tiny_skia::Pixmap::new(4, 4).expect("pixmap");
        for i in 0..4 {
            let px = i * 4;
            // Valid premultiplied red at 50% alpha: colour cannot exceed alpha.
            p.data_mut()[px..px + 4].copy_from_slice(&[128, 0, 0, 128]);
        }
        apply_filter(
            &mut p,
            &Filter {
                opacity: Some(0.5),
                ..Filter::default()
            },
        );
        let px = p.data();
        assert_eq!(px[0], 64, "red should halve: {}", px[0]);
        assert_eq!(px[1], 0, "green should stay 0: {}", px[1]);
        assert_eq!(px[3], 64, "alpha should halve: {}", px[3]);
    }

    #[test]
    fn composite_source_over_an_opaque_destination() {
        // The case a "does the filter run" check cannot see: over an opaque
        // page, an inverted card must actually replace it.
        let mut dst = tiny_skia::Pixmap::new(4, 4).expect("pixmap");
        dst.fill(tiny_skia::Color::WHITE);
        let mut layer = tiny_skia::Pixmap::new(4, 4).expect("pixmap");
        layer.fill(tiny_skia::Color::from_rgba8(191, 127, 63, 255));
        composite(&mut dst, &layer, 0, 0);
        assert_eq!(&dst.data()[..3], &[191, 127, 63], "source was discarded");
    }

    #[test]
    fn composite_is_clipped_at_the_canvas_edge() {
        let mut dst = tiny_skia::Pixmap::new(4, 4).expect("pixmap");
        dst.fill(tiny_skia::Color::WHITE);
        let mut layer = tiny_skia::Pixmap::new(4, 4).expect("pixmap");
        layer.fill(tiny_skia::Color::BLACK);
        composite(&mut dst, &layer, 2, 2);
        // The overlap is 2x2 and must be black; the rest stays white.
        assert_eq!(dst.data()[(3 * 4 + 3) * 4], 0, "overlap should be black");
        assert_eq!(dst.data()[0], 255, "outside should stay white");
    }

    #[test]
    fn composite_of_a_fully_transparent_layer_changes_nothing() {
        let mut dst = tiny_skia::Pixmap::new(4, 4).expect("pixmap");
        dst.fill(tiny_skia::Color::WHITE);
        let layer = tiny_skia::Pixmap::new(4, 4).expect("pixmap");
        let before = dst.data().to_vec();
        composite(&mut dst, &layer, 0, 0);
        assert_eq!(dst.data(), before.as_slice());
    }
}
