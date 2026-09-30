#![warn(missing_docs)]
//! `hikari`: one-API facade over layout + raster + SVG.
//!
//! ```rust
//! use hikari::{Node, render_png};
//! let tree = Node::banner(1200.0, 630.0, "#0b1020", "Hello", 72.0, "#fff");
//! let png = render_png(&tree, 1200, 630).unwrap();
//! assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
//! ```

pub use hikari_animate::{count_gif_frames, encode_apng, encode_gif, AnimFrame};
pub use hikari_core::{
    balance_text, builtin_bytes, compute_layout, fit_font_size, font_bytes, font_entry,
    gradient_line, hash_bytes, hash_node, image_dimensions, line_height, measure_text, paginate,
    register_font, registered_font_count, sample_background, shape_text, wrap_text, Align,
    Background, Color, ColorStop, Display, Error, FlexDir, Flow, FontEntry, FontId, HashCache,
    ImgFit, Justify, Media, Node, Placed, PlacedAdvance, Style, BUILTIN_FONT,
};
pub use hikari_core::{BlendMode, Shadow, ShadowKind};
pub use hikari_license::{Feature, License, LicenseError, Plan};
pub use hikari_pdf::{
    render_pdf as render_pdf_pages, render_pdf_with as render_pdf_pages_with, Attachment, PageSize,
    PdfOptions,
};
pub use hikari_raster::{
    glyph_cache_stats, image_cache_stats, render_to_png as render_placed_to_png,
    render_to_webp as render_placed_to_webp,
};
pub use hikari_svg::render_to_svg as render_placed_to_svg;

/// Reusable renderer with a content-hash PNG cache.
pub struct Renderer {
    cache: HashCache<Vec<u8>>,
}

impl Renderer {
    /// Empty renderer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: HashCache::new(),
        }
    }

    /// Cache hits so far.
    #[must_use]
    pub fn hits(&self) -> u64 {
        self.cache.hits
    }

    /// Cache misses so far.
    #[must_use]
    pub fn misses(&self) -> u64 {
        self.cache.misses
    }

    /// Render PNG, reusing bytes for identical `(tree, w, h)`.
    pub fn render_png_cached(&mut self, tree: &Node, w: u32, h: u32) -> Result<Vec<u8>, Error> {
        let key = hash_node(tree, w, h);
        if let Some(hit) = self.cache.get(&key) {
            return Ok(hit.clone());
        }
        let bytes = render_png(tree, w, h)?;
        self.cache.insert(key, bytes.clone());
        Ok(bytes)
    }
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Layout + rasterize to PNG bytes.
pub fn render_png(tree: &Node, w: u32, h: u32) -> Result<Vec<u8>, Error> {
    let placed = compute_layout(tree, w as f32, h as f32)?;
    hikari_raster::render_to_png(&placed, w, h)
}

/// Layout + encode to lossless WebP bytes.
pub fn render_webp(tree: &Node, w: u32, h: u32) -> Result<Vec<u8>, Error> {
    let placed = compute_layout(tree, w as f32, h as f32)?;
    hikari_raster::render_to_webp(&placed, w, h)
}

/// Layout + serialize to SVG.
pub fn render_svg(tree: &Node, w: u32, h: u32) -> Result<String, Error> {
    let placed = compute_layout(tree, w as f32, h as f32)?;
    Ok(hikari_svg::render_to_svg(&placed, w, h))
}

/// Render animation frames in parallel with `rayon`.
/// Each frame is `(tree, duration_ms)`; output is one PNG per frame.
pub fn render_animation_png(
    frames: &[(Node, u32, u32)],
    w: u32,
    h: u32,
) -> Result<Vec<Vec<u8>>, Error> {
    use rayon::prelude::*;
    frames
        .par_iter()
        .map(|(tree, _, _)| render_png(tree, w, h))
        .collect()
}

/// Current unix time in seconds (license clock).
#[must_use]
pub fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Render frames in parallel to straight-alpha RGBA (one buffer per frame).
pub fn render_animation_rgba(
    frames: &[(Node, u32)],
    w: u32,
    h: u32,
) -> Result<Vec<(Vec<u8>, u32)>, Error> {
    use rayon::prelude::*;
    frames
        .par_iter()
        .map(|(tree, duration_ms)| {
            let placed = compute_layout(tree, w as f32, h as f32)?;
            let rgba = hikari_raster::render_to_rgba(&placed, w, h)?;
            Ok((rgba, *duration_ms))
        })
        .collect()
}

/// Render + encode an animated GIF (Pro: requires `Feature::Animate`).
/// Frames are `(tree, duration_ms)`; output loops forever.
pub fn render_animation_gif(
    frames: &[(Node, u32)],
    w: u32,
    h: u32,
    license: &License,
) -> Result<Vec<u8>, Error> {
    if !license.allows_at(Feature::Animate, now_unix()) {
        return Err(Error::License(LicenseError::NotEntitled.to_string()));
    }
    let raw = render_animation_rgba(frames, w, h)?;
    let anim: Vec<AnimFrame> = raw
        .into_iter()
        .map(|(rgba, duration_ms)| AnimFrame { rgba, duration_ms })
        .collect();
    encode_gif(&anim, w, h)
}

/// Render + encode an animated PNG (Pro: requires `Feature::Animate`).
/// Frames are `(tree, duration_ms)`; output loops forever.
pub fn render_animation_apng(
    frames: &[(Node, u32)],
    w: u32,
    h: u32,
    license: &License,
) -> Result<Vec<u8>, Error> {
    if !license.allows_at(Feature::Animate, now_unix()) {
        return Err(Error::License(LicenseError::NotEntitled.to_string()));
    }
    let raw = render_animation_rgba(frames, w, h)?;
    let anim: Vec<AnimFrame> = raw
        .into_iter()
        .map(|(rgba, duration_ms)| AnimFrame { rgba, duration_ms })
        .collect();
    encode_apng(&anim, w, h)
}

/// Render one [`Node`] per page to PDF bytes (Pro: requires `Feature::Pdf`).
/// Text stays selectable; images embed as Flate XObjects.
pub fn render_pdf(pages: &[Node], size: PageSize, license: &License) -> Result<Vec<u8>, Error> {
    render_pdf_with(pages, size, &PdfOptions::new(), license)
}

/// Render one [`Node`] per page to PDF bytes with document options
/// (Pro: requires `Feature::Pdf`).
pub fn render_pdf_with(
    pages: &[Node],
    size: PageSize,
    options: &PdfOptions,
    license: &License,
) -> Result<Vec<u8>, Error> {
    if !license.allows_at(Feature::Pdf, now_unix()) {
        return Err(Error::License(LicenseError::NotEntitled.to_string()));
    }
    render_pdf_pages_with(pages, size, options)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facade_png_and_svg() {
        let tree = Node::banner(1200.0, 630.0, "#0b1020", "Hikari", 72.0, "#ffffff");
        let png = render_png(&tree, 1200, 630).unwrap();
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let svg = render_svg(&tree, 1200, 630).unwrap();
        assert!(svg.contains("Hikari"));
    }

    #[test]
    fn cache_hits_second_render() {
        let tree = Node::banner(600.0, 300.0, "#111111", "Cached", 48.0, "#ffffff");
        let mut r = Renderer::new();
        let a = r.render_png_cached(&tree, 600, 300).unwrap();
        let b = r.render_png_cached(&tree, 600, 300).unwrap();
        assert_eq!(a, b);
        assert_eq!((r.hits(), r.misses()), (1, 1));
    }

    #[test]
    fn parallel_frames() {
        let mk = |t: &str| Node::banner(320.0, 160.0, "#101828", t, 40.0, "#ffffff");
        let frames = vec![(mk("a"), 500, 0), (mk("b"), 500, 0), (mk("c"), 500, 0)];
        let out = render_animation_png(&frames, 320, 160).unwrap();
        assert_eq!(out.len(), 3);
        assert_ne!(out[0], out[1]);
    }

    #[test]
    fn gif_renders_with_dev_license() {
        let mk = |t: &str| Node::banner(160.0, 80.0, "#101828", t, 32.0, "#ffffff");
        let frames = vec![(mk("one"), 400), (mk("two"), 400)];
        let lic = License::dev(now_unix());
        let gif = render_animation_gif(&frames, 160, 80, &lic).unwrap();
        assert_eq!(&gif[..6], b"GIF89a");
        assert_eq!(count_gif_frames(&gif).unwrap(), 2);
    }

    #[test]
    fn pdf_renders_two_selectable_pages() {
        let mk = |t: &str| Node::banner(400.0, 600.0, "#ffffff", t, 36.0, "#111111");
        let lic = License::dev(now_unix());
        let doc = render_pdf(
            &[mk("Invoice total $288"), mk("Terms apply")],
            PageSize::Custom { w: 400.0, h: 600.0 },
            &lic,
        )
        .unwrap();
        assert_eq!(&doc[..5], b"%PDF-");
        let pages = doc.windows(11).filter(|w| *w == b"/Type /Page").count()
            - doc.windows(12).filter(|w| *w == b"/Type /Pages").count();
        assert_eq!(pages, 2);
        assert!(doc.windows(10).any(|w| *w == *b"/ToUnicode"));
    }

    #[test]
    fn pdf_denies_free_plan() {
        let lic = License::dev(0); // long expired (dev keys live 24h)
        let err = render_pdf(
            &[Node::banner(10.0, 10.0, "#fff", "x", 8.0, "#000")],
            PageSize::A4,
            &lic,
        )
        .unwrap_err();
        assert!(err.to_string().contains("license"), "{err}");
    }

    #[test]
    fn apng_renders_with_dev_license() {
        let mk = |t: &str| Node::banner(160.0, 80.0, "#101828", t, 32.0, "#ffffff");
        let frames = vec![(mk("one"), 400), (mk("two"), 400)];
        let lic = License::dev(now_unix());
        let apng = render_animation_apng(&frames, 160, 80, &lic).unwrap();
        assert_eq!(&apng[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(apng.windows(4).any(|w| w == b"acTL"));
    }

    #[test]
    fn gif_denies_free_plan() {
        use ed25519_dalek::SigningKey;
        const SK: [u8; 32] = [9u8; 32];
        let pk = SigningKey::from_bytes(&SK).verifying_key().to_bytes();
        let key = License::mint(Plan::Free, 0, "free", &SK);
        let lic = License::verify(&key, &pk, now_unix()).unwrap();
        let frames = vec![(Node::banner(64.0, 32.0, "#111", "x", 16.0, "#fff"), 200)];
        let err = render_animation_gif(&frames, 64, 32, &lic).unwrap_err();
        assert!(err.to_string().contains("license"), "{err}");
    }
}
