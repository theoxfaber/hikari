#![warn(missing_docs)]
//! CPU raster backend: `Placed` tree -> PNG bytes via `tiny-skia`.
//!
//! Text is shaped with upstream `rustybuzz` (`hikari-core`) and rasterized
//! with `fontdue` from one embedded `DejaVu Sans`, so layout and paint agree
//! and output is deterministic across machines. Complex-script joining is
//! best-effort in v0.2; per-font fallback is Step 1b on the roadmap.

use std::sync::{Mutex, OnceLock};

use fontdue::{Font, FontSettings};
use hikari_core::{font_bytes, hash_bytes, Error, HashCache, ImgFit, Placed};
use tiny_skia::{BlendMode, FillRule, Mask, Paint, PathBuilder, Pixmap, Transform};

static FONT: OnceLock<Font> = OnceLock::new();
/// System CJK-capable fallback, loaded lazily and only when a glyph is
/// missing from the embedded font. Never bundled (proprietary on macOS);
/// production deployments should ship a subsetted OFL CJK font instead.
static FALLBACK: OnceLock<Option<Font>> = OnceLock::new();

fn font() -> Result<&'static Font, Error> {
    if let Some(f) = FONT.get() {
        return Ok(f);
    }
    let f = Font::from_bytes(font_bytes(), FontSettings::default())
        .map_err(|e| Error::Font(e.to_owned()))?;
    Ok(FONT.get_or_init(|| f))
}

fn fallback_font() -> Option<&'static Font> {
    use hikari_core::fallback_font_bytes;
    FALLBACK
        .get_or_init(|| {
            fallback_font_bytes().and_then(|b| Font::from_bytes(b, FontSettings::default()).ok())
        })
        .as_ref()
}

/// Render a laid-out tree to PNG bytes.
///
/// Encodes with default compression + adaptive filtering, and strips the
/// alpha channel when the frame is fully opaque (the `oxipng` lesson twice:
/// filtering dominates size for flat graphics, and opaque RGB compresses
/// ~25% smaller than RGBA). Repeated identical renders should go through
/// the caller's byte cache.
pub fn render_to_png(placed: &Placed, width: u32, height: u32) -> Result<Vec<u8>, Error> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    let mut pix =
        Pixmap::new(width, height).ok_or_else(|| Error::Raster("pixmap alloc failed".into()))?;
    pix.fill(tiny_skia::Color::WHITE);
    paint_box(placed, &mut pix, 0.0, 0.0)?;
    let (bytes, color) = pixmap_to_bytes(&pix);
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Default, FilterType::Adaptive)
        .write_image(&bytes, width, height, color)
        .map_err(|e| Error::Encode(e.to_string()))?;
    Ok(out)
}

/// Straight-alpha bytes + color type; alpha is stripped when fully opaque.
fn pixmap_to_bytes(pix: &Pixmap) -> (Vec<u8>, image::ExtendedColorType) {
    let mut opaque = true;
    for p in pix.pixels() {
        if p.alpha() != 255 {
            opaque = false;
            break;
        }
    }
    if opaque {
        let mut out = Vec::with_capacity(pix.pixels().len() * 3);
        for p in pix.pixels() {
            // Un-premultiply defensively (alpha is 255 here, so exact).
            let a = u32::from(p.alpha()).max(1);
            let un = |c: u32| ((c * 255 + a / 2) / a).min(255) as u8;
            out.extend_from_slice(&[
                un(u32::from(p.red())),
                un(u32::from(p.green())),
                un(u32::from(p.blue())),
            ]);
        }
        (out, image::ExtendedColorType::Rgb8)
    } else {
        (unpremultiply(pix), image::ExtendedColorType::Rgba8)
    }
}

/// Straight-alpha RGBA bytes from premultiplied storage.
fn unpremultiply(pix: &Pixmap) -> Vec<u8> {
    let mut out = Vec::with_capacity(pix.pixels().len() * 4);
    for p in pix.pixels() {
        let (a, r, g, b) = (
            u32::from(p.alpha()),
            u32::from(p.red()),
            u32::from(p.green()),
            u32::from(p.blue()),
        );
        if a == 0 {
            out.extend_from_slice(&[0, 0, 0, 0]);
        } else {
            // Round to nearest while un-premultiplying.
            let un = |c: u32| ((c * 255 + a / 2) / a).min(255) as u8;
            out.extend_from_slice(&[un(r), un(g), un(b), a as u8]);
        }
    }
    out
}

/// Render a laid-out tree to lossless WebP bytes (VP8L, pure Rust).
pub fn render_to_webp(placed: &Placed, width: u32, height: u32) -> Result<Vec<u8>, Error> {
    use image::codecs::webp::WebPEncoder;
    use image::ExtendedColorType;
    let rgba = render_to_rgba(placed, width, height)?;
    let mut out = Vec::new();
    WebPEncoder::new_lossless(&mut out)
        .encode(&rgba, width, height, ExtendedColorType::Rgba8)
        .map_err(|e| Error::Encode(e.to_string()))?;
    Ok(out)
}

/// Render to straight-alpha RGBA bytes (for animation encoders).
/// `tiny-skia` stores premultiplied pixels; this un-premultiplies them.
pub fn render_to_rgba(placed: &Placed, width: u32, height: u32) -> Result<Vec<u8>, Error> {
    let mut pix =
        Pixmap::new(width, height).ok_or_else(|| Error::Raster("pixmap alloc failed".into()))?;
    pix.fill(tiny_skia::Color::WHITE);
    paint_box(placed, &mut pix, 0.0, 0.0)?;
    Ok(unpremultiply(&pix))
}

/// Drop shadow: blurred silhouette of the (spread-expanded) box, tinted and
/// composited beneath the box. Three box-blur passes approximate a gaussian.
fn paint_shadow(pix: &mut Pixmap, node: &Placed, x: f32, y: f32, shadow: &hikari_core::Shadow) {
    let (sw, sh) = (
        (node.w + 2.0 * shadow.spread).max(1.0),
        (node.h + 2.0 * shadow.spread).max(1.0),
    );
    let margin = (shadow.blur * 2.0).ceil().max(2.0) as u32 as i32;
    let (tw, th) = (
        (sw.ceil() as i32 + 2 * margin).max(1),
        (sh.ceil() as i32 + 2 * margin).max(1),
    );
    let mut tmp = match Pixmap::new(tw as u32, th as u32) {
        Some(p) => p,
        None => return,
    };
    fill_solid(
        &mut tmp,
        margin as f32,
        margin as f32,
        sw,
        sh,
        node.style.radius + shadow.spread,
        hikari_core::Color::rgb(255, 255, 255),
    );
    if shadow.blur > 0.5 {
        blur_bytes(
            tmp.data_mut(),
            tw as usize,
            th as usize,
            shadow.blur.ceil() as usize,
        );
    }
    // Reuse alpha compositing: blurred alpha becomes coverage, tint = color.
    let cov: Vec<u8> = tmp.data().iter().skip(3).step_by(4).copied().collect();
    blit_alpha(
        pix,
        &cov,
        tw as usize,
        th as usize,
        (x - shadow.spread - margin as f32 + shadow.dx).round() as i32,
        (y - shadow.spread - margin as f32 + shadow.dy).round() as i32,
        shadow.color,
    );
}

/// Three separable box-blur passes over premultiplied RGBA bytes.
fn blur_bytes(buf: &mut [u8], w: usize, h: usize, radius: usize) {
    if radius == 0 || w == 0 || h == 0 {
        return;
    }
    let mut scratch = vec![0u8; buf.len()];
    for _ in 0..3 {
        blur_pass(buf, &mut scratch, w, h, radius, true);
        blur_pass(&scratch, buf, w, h, radius, false);
    }
}

fn blur_pass(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize, horizontal: bool) {
    let (outer, inner) = if horizontal { (h, w) } else { (w, h) };
    let at = |o: usize, i: usize| -> usize {
        if horizontal {
            (o * w + i) * 4
        } else {
            (i * w + o) * 4
        }
    };
    for o in 0..outer {
        // Window [lo, hi] for output i, edges clamped.
        let mut sum = [0u32; 4];
        let mut lo = 0usize;
        let mut hi = r.min(inner - 1);
        for i in lo..=hi {
            let p = at(o, i);
            for c in 0..4 {
                sum[c] += u32::from(src[p + c]);
            }
        }
        for i in 0..inner {
            let p = at(o, i);
            let n = (hi - lo + 1) as u32;
            for c in 0..4 {
                dst[p + c] = (sum[c] / n) as u8;
            }
            let nlo = (i + 1).saturating_sub(r);
            let nhi = (i + 1 + r).min(inner - 1);
            while lo < nlo {
                let d = at(o, lo);
                for c in 0..4 {
                    sum[c] -= u32::from(src[d + c]);
                }
                lo += 1;
            }
            while hi < nhi {
                hi += 1;
                let a = at(o, hi);
                for c in 0..4 {
                    sum[c] += u32::from(src[a + c]);
                }
            }
        }
    }
}
fn paint_box(node: &Placed, pix: &mut Pixmap, dx: f32, dy: f32) -> Result<(), Error> {
    use hikari_core::Media;
    let x = dx + node.x;
    let y = dy + node.y;
    // Shadow beneath everything else.
    if let Some(shadow) = node.style.shadow {
        paint_shadow(pix, node, x, y, &shadow);
    }
    // Border ring first, then background inset by the border width.
    if node.style.border > 0.0 {
        let bc = node
            .style
            .border_color
            .unwrap_or(hikari_core::Color::rgb(0, 0, 0));
        fill_solid(pix, x, y, node.w, node.h, node.style.radius, bc);
    }
    // `background-clip: text` consumes the background as glyph fill.
    let clipped = node.text.is_some() && node.style.clip_text && node.style.background.is_some();
    if let Some(bg) = &node.style.background {
        if !clipped {
            let b = node.style.border;
            let inset_r = (node.style.radius - b).max(0.0);
            paint_background(
                pix,
                bg,
                x + b,
                y + b,
                node.w - 2.0 * b,
                node.h - 2.0 * b,
                inset_r,
            );
        }
    }
    if let Some(Media::Image { bytes, fit }) = &node.media {
        draw_image(bytes, *fit, node, pix, x, y)?;
    }
    if let Some(text) = &node.text {
        draw_text(text, node, pix, x, y)?;
    }
    for child in &node.children {
        paint_box(child, pix, x, y)?;
    }
    Ok(())
}

/// Fill a (possibly rounded) rect with a solid color; no-op on degenerate boxes.
fn fill_solid(pix: &mut Pixmap, x: f32, y: f32, w: f32, h: f32, r: f32, c: hikari_core::Color) {
    let (cr, cg, cb, ca) = c.to_rgba_f32();
    let shader = tiny_skia::Shader::SolidColor(
        tiny_skia::Color::from_rgba(cr, cg, cb, ca).unwrap_or(tiny_skia::Color::BLACK),
    );
    fill_shader(pix, x, y, w, h, r, shader);
}

/// Paint any background (solid or gradient) clipped to a rounded rect.
fn paint_background(
    pix: &mut Pixmap,
    bg: &hikari_core::Background,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
) {
    use hikari_core::Background;
    match bg {
        Background::Solid(c) => fill_solid(pix, x, y, w, h, r, *c),
        Background::Linear { angle_deg, stops } => {
            let ((x0, y0), (x1, y1)) = hikari_core::gradient_line(*angle_deg, x, y, w, h);
            let shader = tiny_skia::LinearGradient::new(
                tiny_skia::Point::from_xy(x0, y0),
                tiny_skia::Point::from_xy(x1, y1),
                sk_stops(stops),
                tiny_skia::SpreadMode::Pad,
                Transform::identity(),
            );
            match shader {
                Some(s) => fill_shader(pix, x, y, w, h, r, s),
                None => {
                    if let Some(first) = stops.first() {
                        fill_solid(pix, x, y, w, h, r, first.color);
                    }
                }
            }
        }
        Background::Radial {
            cx,
            cy,
            radius,
            stops,
        } => {
            let (ccx, ccy) = (x + cx * w, y + cy * h);
            let rad = if *radius > 0.0 {
                *radius
            } else {
                (w * w + h * h).sqrt() / 2.0
            };
            let shader = tiny_skia::RadialGradient::new(
                tiny_skia::Point::from_xy(ccx, ccy),
                tiny_skia::Point::from_xy(ccx, ccy),
                rad,
                sk_stops(stops),
                tiny_skia::SpreadMode::Pad,
                Transform::identity(),
            );
            match shader {
                Some(s) => fill_shader(pix, x, y, w, h, r, s),
                None => {
                    if let Some(first) = stops.first() {
                        fill_solid(pix, x, y, w, h, r, first.color);
                    }
                }
            }
        }
    }
}

fn sk_stops(stops: &[hikari_core::ColorStop]) -> Vec<tiny_skia::GradientStop> {
    stops
        .iter()
        .map(|s| {
            let (r, g, b, a) = s.color.to_rgba_f32();
            tiny_skia::GradientStop::new(
                s.pos,
                tiny_skia::Color::from_rgba(r, g, b, a).unwrap_or(tiny_skia::Color::BLACK),
            )
        })
        .collect()
}

/// Fill a rounded rect with an arbitrary shader.
fn fill_shader(
    pix: &mut Pixmap,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    shader: tiny_skia::Shader<'_>,
) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    if let Some(path) = rounded_rect(x, y, w, h, r) {
        let paint = Paint {
            shader,
            blend_mode: BlendMode::SourceOver,
            anti_alias: true,
            ..Default::default()
        };
        pix.fill_path(
            &path,
            &paint,
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }
}

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Option<tiny_skia::Path> {
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let r = r.clamp(0.0, (w.min(h)) / 2.0);
    let mut pb = PathBuilder::new();
    if r <= 0.5 {
        if let Some(rect) = tiny_skia::Rect::from_xywh(x, y, w, h) {
            pb.push_rect(rect);
        }
        return pb.finish();
    }
    pb.move_to(x + r, y);
    pb.line_to(x + w - r, y);
    pb.quad_to(x + w, y, x + w, y + r);
    pb.line_to(x + w, y + h - r);
    pb.quad_to(x + w, y + h, x + w - r, y + h);
    pb.line_to(x + r, y + h);
    pb.quad_to(x, y + h, x, y + h - r);
    pb.line_to(x, y + r);
    pb.quad_to(x, y, x + r, y);
    pb.close();
    pb.finish()
}

/// Decoded-image cache keyed by content hash (Step 3 asset pipeline).
static IMAGES: OnceLock<Mutex<HashCache<Pixmap>>> = OnceLock::new();

/// Cache hits/misses for decoded images (tested, reported in benches).
#[must_use]
pub fn image_cache_stats() -> (u64, u64) {
    let c = IMAGES.get_or_init(|| Mutex::new(HashCache::new()));
    let c = lock_cache(c);
    (c.hits, c.misses)
}

fn decoded_image(bytes: &[u8]) -> Result<Pixmap, Error> {
    let key = hash_bytes(bytes);
    let lock = IMAGES.get_or_init(|| Mutex::new(HashCache::new()));
    let mut cache = lock.lock().map_err(|e| Error::Raster(e.to_string()))?;
    if let Some(hit) = cache.get(&key) {
        return Ok(hit.clone());
    }
    let img = image::load_from_memory(bytes).map_err(|e| Error::Asset(e.to_string()))?;
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let size = tiny_skia::IntSize::from_wh(w, h)
        .ok_or_else(|| Error::Raster("image size failed".into()))?;
    let pix = Pixmap::from_vec(rgba.into_raw(), size)
        .ok_or_else(|| Error::Raster("image alloc failed".into()))?;
    cache.insert(key, pix.clone());
    Ok(pix)
}

/// Paint an image leaf with cover/contain/fill semantics + rounded clipping.
fn draw_image(
    bytes: &[u8],
    fit: ImgFit,
    node: &Placed,
    pix: &mut Pixmap,
    bx: f32,
    by: f32,
) -> Result<(), Error> {
    // Inset into the border ring so images never cover it.
    let bi = node.style.border;
    let (bx, by) = (bx + bi, by + bi);
    let bw = (node.w - 2.0 * bi).max(1.0);
    let bh = (node.h - 2.0 * bi).max(1.0);
    let src = decoded_image(bytes)?;
    let (sw, sh) = (src.width().max(1) as f32, src.height().max(1) as f32);
    // Source crop (cover) or full frame, then scale to the destination box.
    let (crop_x, crop_y, crop_w, crop_h, dx, dy, dw, dh) = match fit {
        ImgFit::Fill => (0.0, 0.0, sw, sh, bx, by, bw, bh),
        ImgFit::Cover => {
            let s = (bw / sw).max(bh / sh);
            let cw = bw / s;
            let ch = bh / s;
            ((sw - cw) / 2.0, (sh - ch) / 2.0, cw, ch, bx, by, bw, bh)
        }
        ImgFit::Contain => {
            let s = (bw / sw).min(bh / sh);
            let dw = sw * s;
            let dh = sh * s;
            (
                0.0,
                0.0,
                sw,
                sh,
                bx + (bw - dw) / 2.0,
                by + (bh - dh) / 2.0,
                dw,
                dh,
            )
        }
    };
    let dw_px = dw.round().max(1.0) as u32;
    let dh_px = dh.round().max(1.0) as u32;
    // Clamp the crop to the source frame (rounding must never escape it).
    let cx = crop_x.clamp(0.0, sw - 1.0);
    let cy = crop_y.clamp(0.0, sh - 1.0);
    let cw = crop_w.clamp(1.0, sw - cx);
    let ch = crop_h.clamp(1.0, sh - cy);
    // Crop + resize on the CPU via `image`, then blit as one pixmap.
    let src_rgba = image::RgbaImage::from_raw(src.width(), src.height(), src.data().to_vec())
        .ok_or_else(|| Error::Raster("image re-read failed".into()))?;
    let cropped =
        image::imageops::crop_imm(&src_rgba, cx as u32, cy as u32, cw as u32, ch as u32).to_image();
    let resized = image::imageops::resize(
        &cropped,
        dw_px,
        dh_px,
        image::imageops::FilterType::Lanczos3,
    );
    let tile_size = tiny_skia::IntSize::from_wh(dw_px, dh_px)
        .ok_or_else(|| Error::Raster("tile size failed".into()))?;
    let tile = Pixmap::from_vec(resized.into_raw(), tile_size)
        .ok_or_else(|| Error::Raster("tile alloc failed".into()))?;
    let paint = tiny_skia::PixmapPaint {
        opacity: 1.0,
        blend_mode: BlendMode::SourceOver,
        quality: tiny_skia::FilterQuality::Bilinear,
    };
    let mask = if node.style.radius > 0.5 {
        let inset_r = (node.style.radius - node.style.border).max(0.0);
        rounded_rect(bx, by, bw, bh, inset_r).and_then(|path| {
            let mut m = Mask::new(pix.width(), pix.height())?;
            m.fill_path(&path, FillRule::Winding, true, Transform::identity());
            Some(m)
        })
    } else {
        None
    };
    pix.draw_pixmap(
        dx.round() as i32,
        dy.round() as i32,
        tile.as_ref(),
        &paint,
        Transform::identity(),
        mask.as_ref(),
    );
    Ok(())
}

/// Glyph bitmap cache: `fontdue` rasterization is the dominant paint cost,
/// so repeated glyphs (headlines, tables, invoices) hit this instead.
/// Bounded by clear-on-full (steady-state docs reuse the same glyphs).
#[derive(Clone)]
struct GlyphEntry {
    w: usize,
    h: usize,
    xmin: f32,
    ymin: f32,
    height: f32,
    advance: f32,
    bitmap: Vec<u8>,
}

static GLYPHS: OnceLock<Mutex<HashCache<GlyphEntry>>> = OnceLock::new();

const MAX_GLYPHS: usize = 4096;

/// Cache hits/misses for glyph bitmaps.
#[must_use]
pub fn glyph_cache_stats() -> (u64, u64) {
    let c = GLYPHS.get_or_init(|| Mutex::new(HashCache::new()));
    let c = lock_cache(c);
    (c.hits, c.misses)
}

/// Lock a cache, recovering from poisoning.
///
/// These caches are pure memoization: a lost race or a panic inside some other
/// caller's frame must not turn into a permanent failure. With
/// `lock().expect(...)` a single panic while the lock was held poisons the
/// mutex, and then *every* later render panics too -- one bad frame turns into
/// a dead process. The guard is always dropped before any fallible work, so the
/// contents are consistent enough to keep using; at worst an entry is
/// recomputed.
fn lock_cache<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Which glyph to rasterize, and how to name it in the cache.
#[derive(Clone, Copy)]
enum GlyphRef {
    /// A glyph the shaper already resolved, in the primary font.
    Id(u16),
    /// A character looked up directly. Only valid for the fallback font,
    /// which never went through the shaper.
    Char(char),
}

fn rasterize_cached(font_id: u8, font: &Font, glyph: GlyphRef, px: f32) -> GlyphEntry {
    // Keyed on the shaped glyph id, not the source character: after shaping,
    // one character can become a ligature glyph and one glyph can stand in for
    // several characters, so the character is not a stable cache key.
    let key = match glyph {
        GlyphRef::Id(gid) => format!("{font_id}:g{gid}:{}", px.to_bits()),
        GlyphRef::Char(ch) => format!("{font_id}:c{ch}:{}", px.to_bits()),
    };
    let lock = GLYPHS.get_or_init(|| Mutex::new(HashCache::new()));
    let mut cache = lock_cache(lock);
    if let Some(hit) = cache.get(&key) {
        return hit.clone();
    }
    let (m, bmp) = match glyph {
        GlyphRef::Id(gid) => font.rasterize_indexed(gid, px),
        GlyphRef::Char(ch) => font.rasterize(ch, px),
    };
    let entry = GlyphEntry {
        w: m.width,
        h: m.height,
        xmin: m.bounds.xmin,
        ymin: m.bounds.ymin,
        height: m.bounds.height,
        advance: m.advance_width.max(1.0),
        bitmap: bmp,
    };
    if cache.len() >= MAX_GLYPHS {
        // Crude bound: steady-state documents reuse glyphs, so a full
        // clear costs one cold pass and keeps memory flat forever.
        *cache = HashCache::new();
    }
    cache.insert(key, entry.clone());
    entry
}

fn draw_text(text: &str, node: &Placed, pix: &mut Pixmap, bx: f32, by: f32) -> Result<(), Error> {
    use hikari_core::{line_height, sample_background, shape_text};
    let px = node.style.font_size.unwrap_or(16.0).max(1.0);
    let fg = node.style.color.unwrap_or(hikari_core::Color::rgb(0, 0, 0));
    // `background-clip: text`: sample the background per glyph instead.
    let clip = node
        .style
        .clip_text
        .then(|| node.style.background.clone())
        .flatten();
    let lh = line_height(px);
    let lines: Vec<&str> = text.split('\n').collect();
    let total_h = lines.len() as f32 * lh;
    // Vertically center the block; center each line horizontally (shaped order).
    let mut baseline = by + ((node.h - total_h) / 2.0).max(0.0) + px * 0.9;
    for line in lines {
        let (advances, total) = shape_text(line, px);
        let mut cx = bx + ((node.w - total) / 2.0).max(0.0);
        for adv in advances {
            let draw_x = cx + adv.x_offset;
            // Per-glyph fallback: embedded font first, system CJK second.
            let (font_id, glyph_font) = if adv.missing {
                match fallback_font().filter(|fb| fb.has_glyph(adv.ch)) {
                    Some(fb) => (1u8, fb),
                    None => (0, font()?),
                }
            } else {
                (0, font()?)
            };
            let mut glyph_adv = adv.advance;
            if adv.ch != ' ' && !adv.ch.is_control() {
                // The primary font is addressed by the id the shaper chose.
                // The fallback font never went through the shaper, so it is
                // addressed by character instead.
                let target = if font_id == 1 {
                    GlyphRef::Char(adv.ch)
                } else {
                    GlyphRef::Id(adv.gid as u16)
                };
                let g = rasterize_cached(font_id, glyph_font, target, px);
                if font_id == 1 {
                    glyph_adv = g.advance;
                }
                let gx = draw_x + g.xmin;
                let gy = baseline - g.ymin - g.height;
                // Clip-text samples the background at the glyph center.
                let color = match &clip {
                    Some(bg) => sample_background(
                        bg,
                        node.w,
                        node.h,
                        (draw_x + glyph_adv / 2.0 - bx).max(0.0),
                        (baseline - by - px * 0.2).clamp(0.0, node.h.max(1.0)),
                    ),
                    None => fg,
                };
                blit_alpha(
                    pix,
                    &g.bitmap,
                    g.w,
                    g.h,
                    gx.round() as i32,
                    gy.round() as i32,
                    color,
                );
            }
            cx += glyph_adv;
        }
        baseline += lh;
    }
    Ok(())
}

fn blit_alpha(
    pix: &mut Pixmap,
    cov: &[u8],
    gw: usize,
    gh: usize,
    gx: i32,
    gy: i32,
    fg: hikari_core::Color,
) {
    if gw == 0 || gh == 0 {
        return;
    }
    let pw = pix.width() as i32;
    let ph = pix.height() as i32;
    let stride = pix.width();
    let pixels = pix.pixels_mut();
    for row in 0..gh {
        for col in 0..gw {
            let a = cov[row * gw + col] as u32;
            if a == 0 {
                continue;
            }
            let x = gx + col as i32;
            let y = gy + row as i32;
            if x < 0 || y < 0 || x >= pw || y >= ph {
                continue;
            }
            let idx = (y as u32 * stride + x as u32) as usize;
            let dst = pixels[idx];
            // Source-over with premultiplied storage.
            let sa = a * u32::from(fg.a) / 255;
            let sr = u32::from(fg.r) * sa / 255;
            let sg = u32::from(fg.g) * sa / 255;
            let sb = u32::from(fg.b) * sa / 255;
            let inv = 255 - sa;
            let or_ = (sr + u32::from(dst.red()) * inv / 255).min(255) as u8;
            let og = (sg + u32::from(dst.green()) * inv / 255).min(255) as u8;
            let ob = (sb + u32::from(dst.blue()) * inv / 255).min(255) as u8;
            let oa = (sa + u32::from(dst.alpha()) * inv / 255).min(255) as u8;
            pixels[idx] =
                tiny_skia::PremultipliedColorU8::from_rgba(or_, og, ob, oa).unwrap_or(dst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hikari_core::{compute_layout, Node, Style};

    #[test]
    fn glyph_cache_hits_on_repeat() {
        let tree = Node::banner(400.0, 200.0, "#123456", "aaa bbb aaa", 48.0, "#ffffff");
        let placed = compute_layout(&tree, 400.0, 200.0).unwrap();
        let (h0, _) = glyph_cache_stats();
        render_to_png(&placed, 400, 200).unwrap();
        let (h1, _) = glyph_cache_stats();
        render_to_png(&placed, 400, 200).unwrap();
        let (h2, _) = glyph_cache_stats();
        assert!(h1 >= h0, "first render populates");
        assert!(h2 > h1, "second render hits glyph cache");
    }

    #[test]
    fn webp_roundtrip() {
        let tree = Node::banner(400.0, 200.0, "#123456", "Hi WebP", 48.0, "#ffffff");
        let placed = compute_layout(&tree, 400.0, 200.0).unwrap();
        let bytes = render_to_webp(&placed, 400, 200).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WEBP");
        // Decode back: dimensions + exact background pixel (lossless).
        let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::WebP).unwrap();
        assert_eq!((img.width(), img.height()), (400, 200));
        let rgba = img.to_rgba8();
        assert_eq!(*rgba.get_pixel(5, 5), image::Rgba([0x12, 0x34, 0x56, 255]));
    }

    #[test]
    fn clip_text_paints_gradient_glyphs() {
        // Note: clip flag lives on the box that carries the background.
        let mk = |clip: bool| {
            let mut ts = Style::text(64.0, "#ff0000")
                .with_linear_gradient(90.0, &[(0.0, "#000000"), (1.0, "#ffffff")]);
            if clip {
                ts = ts.clip_text();
            }
            let tree = Node::container(
                Style::new()
                    .with_size(400.0, 120.0)
                    .with_background("#222222"),
                vec![Node::text("Hi", ts)],
            );
            let placed = compute_layout(&tree, 400.0, 120.0).unwrap();
            render_to_png(&placed, 400, 120).unwrap()
        };
        let (solid, clipped) = (mk(false), mk(true));
        assert_eq!(&clipped[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_ne!(solid, clipped, "clip must change glyph fill");
    }

    #[test]
    fn png_signature_and_size() {
        let tree = Node::banner(400.0, 200.0, "#123456", "Hi", 48.0, "#ffffff");
        let placed = compute_layout(&tree, 400.0, 200.0).unwrap();
        let bytes = render_to_png(&placed, 400, 200).unwrap();
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(bytes.len() > 1000);
    }

    #[test]
    fn multilingual_and_multiline_paint() {
        for text in [
            "Hello from Hikari",
            "שלום",
            "مرحبا",
            "日本語",
            "line one\nline two",
        ] {
            let tree = Node::banner(600.0, 300.0, "#0b1020", text, 56.0, "#ffffff");
            let placed = compute_layout(&tree, 600.0, 300.0).unwrap();
            let bytes = render_to_png(&placed, 600, 300).unwrap();
            assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10], "{text}");
            assert!(bytes.len() > 2000, "{text}: {} bytes", bytes.len());
        }
    }

    #[test]
    fn shaped_glyph_ids_reach_the_rasterizer() {
        // Regression guard. The painter used to look glyphs up by *character*
        // rather than by the id the shaper produced, so anything GSUB rewrote --
        // ligatures, Arabic presentation forms -- was asked for by a character
        // that maps to a different, empty glyph and painted as a blank gap.
        // Shaping and measurement were correct the whole time, which is exactly
        // why the bug was invisible to every existing test.
        //
        // The tell is not the amount of ink -- it barely changes -- it is a hole
        // in the middle of a word. So this measures the horizontal ink profile
        // and asserts no run of empty columns inside the text is wide enough to
        // be a missing glyph.
        use hikari_core::Node;

        /// Widest run of empty columns inside the rendered text, in pixels.
        fn widest_gap(text: &str) -> u32 {
            let tree = Node::banner(900.0, 220.0, "#000000", text, 72.0, "#ffffff");
            let placed = hikari_core::compute_layout(&tree, 900.0, 220.0).unwrap();
            let bytes = render_to_png(&placed, 900, 220).unwrap();
            let img = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                .unwrap()
                .to_luma8();
            let (w, h) = (img.width(), img.height());

            // Column ink profile, restricted to rows that contain any ink.
            let rows_with_ink: Vec<u32> = (0..h)
                .filter(|&y| (0..w).any(|x| img.get_pixel(x, y)[0] > 40))
                .collect();
            assert!(!rows_with_ink.is_empty(), "{text:?} painted nothing");
            let (top, bottom) = (rows_with_ink[0], rows_with_ink[rows_with_ink.len() - 1]);

            let mut profile = vec![0u32; w as usize];
            for y in top..=bottom {
                for x in 0..w {
                    if img.get_pixel(x, y)[0] > 40 {
                        profile[x as usize] += 1;
                    }
                }
            }

            let first = profile.iter().position(|&v| v > 0).expect("ink");
            let last = profile.iter().rposition(|&v| v > 0).expect("ink");

            let mut widest = 0u32;
            let mut run = 0u32;
            for &v in &profile[first..=last] {
                if v == 0 {
                    run += 1;
                    widest = widest.max(run);
                } else {
                    run = 0;
                }
            }
            widest
        }

        // "office" -> ffi ligature, "waffle" -> ffl. With the wrong lookup these
        // leave a hole roughly one glyph wide.
        // Threshold chosen from measurement, not taste: correct rendering
        // leaves at most a 12px side-bearing gap, while the char-lookup bug
        // leaves 47px where a ligature should be.
        const MAX_GAP: u32 = 20;
        for text in ["office", "waffle", "flagship", "affix"] {
            let gap = widest_gap(text);
            assert!(
                gap < MAX_GAP,
                "{text:?} has a {gap}px hole inside the text -- a shaped glyph \
                 is not reaching the rasterizer (ligature rendered blank)"
            );
        }

        // Arabic: joined letterforms only appear if the presentation-form
        // glyphs survived both subsetting and the paint path.
        let gap = widest_gap("\u{0645}\u{0631}\u{062D}\u{0628}\u{0627}");
        assert!(
            gap < MAX_GAP,
            "Arabic has a {gap}px hole -- joining forms are not being painted"
        );
    }

    #[test]
    fn image_paints_and_cache_hits() {
        use hikari_core::{ImgFit, Media};
        let bytes = red_png(24, 12);
        let style = hikari_core::Style::new().with_size(120.0, 60.0);
        let placed = hikari_core::Placed {
            x: 0.0,
            y: 0.0,
            w: 120.0,
            h: 60.0,
            style,
            text: None,
            link: None,
            media: Some(Media::Image {
                bytes,
                fit: ImgFit::Cover,
            }),
            children: Vec::new(),
        };
        let (h0, _) = image_cache_stats();
        let a = render_to_png(&placed, 120, 60).unwrap();
        let (h1, _) = image_cache_stats();
        let b = render_to_png(&placed, 120, 60).unwrap();
        let (h2, _) = image_cache_stats();
        assert_eq!(a, b);
        assert!(a.len() > 100);
        assert!(h1 >= h0, "cache populated");
        assert!(h2 > h1, "second render hits cache");
    }

    #[test]
    fn border_and_contain_paint() {
        use hikari_core::{ImgFit, Media};
        let bytes = red_png(10, 20);
        let style = hikari_core::Style::new()
            .with_size(100.0, 100.0)
            .with_border(6.0, "#00ff00")
            .with_background("#000000");
        let placed = hikari_core::Placed {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
            style,
            text: None,
            link: None,
            media: Some(Media::Image {
                bytes,
                fit: ImgFit::Contain,
            }),
            children: Vec::new(),
        };
        let bytes = render_to_png(&placed, 100, 100).unwrap();
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(bytes.len() > 100);
    }

    #[test]
    fn shadow_paints_and_blurs() {
        let base = hikari_core::Style::new()
            .with_size(120.0, 80.0)
            .with_background("#ffffff");
        let plain = Node::container(base.clone(), vec![]);
        let shadowed = Node::container(
            Style {
                shadow: Some(hikari_core::Shadow {
                    dx: 8.0,
                    dy: 8.0,
                    blur: 12.0,
                    spread: 0.0,
                    color: hikari_core::Color::rgb(0, 0, 0),
                }),
                ..base.clone()
            },
            vec![],
        );
        let paint = |t: &Node| {
            let placed = compute_layout(t, 300.0, 200.0).unwrap();
            render_to_png(&placed, 300, 200).unwrap()
        };
        let (a, b) = (paint(&plain), paint(&shadowed));
        assert_eq!(&b[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert_ne!(a, b, "shadow must change output");
        // Sharp (blur 0) shadow also paints.
        let sharp = Node::container(
            Style {
                shadow: Some(hikari_core::Shadow {
                    dx: 6.0,
                    dy: 6.0,
                    blur: 0.0,
                    spread: 2.0,
                    color: hikari_core::Color::rgb(0, 0, 0),
                }),
                ..base.clone()
            },
            vec![],
        );
        assert_ne!(a, paint(&sharp));
    }

    #[test]
    fn gradients_paint_and_differ_from_solid() {
        let grad = hikari_core::Style::new()
            .with_size(200.0, 100.0)
            .with_linear_gradient(180.0, &[(0.0, "#dbeafe"), (1.0, "#fee2e2")]);
        let solid = hikari_core::Style::new()
            .with_size(200.0, 100.0)
            .with_background("#dbeafe");
        let radial = hikari_core::Style::new()
            .with_size(200.0, 100.0)
            .with_radial_gradient(0.5, 0.5, 0.0, &[(0.0, "#ffffff"), (1.0, "#0b1020")]);
        let paint = |s| {
            let tree = Node::container(s, vec![]);
            let placed = compute_layout(&tree, 200.0, 100.0).unwrap();
            render_to_png(&placed, 200, 100).unwrap()
        };
        let (g, s, r) = (paint(grad), paint(solid), paint(radial));
        for b in [&g, &s, &r] {
            assert_eq!(&b[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
            assert!(b.len() > 100);
        }
        assert_ne!(g, s, "gradient must differ from solid fill");
    }

    /// Solid red PNG bytes.
    fn red_png(w: u32, h: u32) -> Vec<u8> {
        use image::codecs::png::PngEncoder;
        use image::{ExtendedColorType, ImageEncoder, RgbaImage};
        let img = RgbaImage::from_pixel(w, h, image::Rgba([200, 30, 30, 255]));
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(img.as_raw(), w, h, ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }
}
