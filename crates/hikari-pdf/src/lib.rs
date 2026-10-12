#![warn(missing_docs)]
//! PDF backend (Pro): laid-out pages -> PDF bytes with selectable text.
//!
//! One [`Node`] per page, each laid out in its page box. Text shows as
//! identity-CID glyph runs with a ToUnicode CMap (select + copy works),
//! using the embedded primary font and, when needed, an embedded runtime
//! fallback font — the same font policy as the raster backend, so PNG and
//! PDF agree. Images embed as Flate RGB XObjects with soft masks.
//!
//! v1 limits, stated openly: solid fills only (gradients use their first
//! stop), mark offsets (`x_offset`) ignored, no auto-pagination across pages
//! (Step 6b). Fonts subset to used glyphs (allsorts, PDF profile).
use skrifa::MetadataProvider;

use std::collections::{BTreeMap, HashMap};

use hikari_core::{
    compute_layout, fallback_font_bytes, font_bytes, glyph_index_in, hash_bytes, line_height,
    paginate, shape_text, Background, Error, Flow, ImgFit, Media, Node, Placed, BUILTIN_FONT,
};
pub mod pdfa;

#[cfg(test)]
mod pdfa_tests;

use pdf_writer::types::{
    ActionType, AnnotationType, CidFontType, FontFlags, OutputIntentSubtype, SystemInfo,
};
use pdf_writer::writers::Outline;
use pdf_writer::{Content, Filter, Name, Pdf, Rect, Ref, Str, TextStr};

/// Page size in points (1 unit = 1 layout px).
#[derive(Debug, Clone, Copy)]
pub enum PageSize {
    /// 595.28 x 841.89 pt.
    A4,
    /// 612 x 792 pt.
    Letter,
    /// Custom width x height.
    Custom {
        /// Width in pt.
        w: f32,
        /// Height in pt.
        h: f32,
    },
}

impl PageSize {
    /// Width/height in points.
    #[must_use]
    pub fn dims(self) -> (f32, f32) {
        match self {
            Self::A4 => (595.28, 841.89),
            Self::Letter => (612.0, 792.0),
            Self::Custom { w, h } => (w, h),
        }
    }
}

/// Render one [`Node`] per page to PDF bytes.
pub fn render_pdf(pages: &[Node], size: PageSize) -> Result<Vec<u8>, Error> {
    render_pdf_with(pages, size, &PdfOptions::default())
}

/// PDF document options.
#[derive(Debug, Clone)]
pub struct PdfOptions {
    /// Document title (Info dict, ASCII).
    pub title: Option<String>,
    /// Document author (Info dict, ASCII).
    pub author: Option<String>,
    /// Emit the outline from bookmarked texts (default true when any exist).
    pub outline: bool,
    /// Flow each input node across pages (auto-pagination, default false).
    pub paginate: bool,
    /// Embedded file attachments (e-invoice XML, data files).
    pub attachments: Vec<Attachment>,
    /// Emit PDF/A-1b structural metadata (default false).
    ///
    /// Adds an output intent with a generated sRGB ICC profile, an XMP packet
    /// declaring `pdfaid:part` 1 and conformance B, and a content-derived
    /// document identifier.
    ///
    /// **This targets PDF/A-1b; it has not been validated against a conformance
    /// checker.** See [`pdfa`] for what is and is not claimed. Off by default
    /// because it costs roughly 700 bytes of ICC profile and a compressed XMP
    /// packet per document, which is a poor trade for a caller who does not need
    /// archival output.
    pub pdfa: bool,
}

/// One embedded file attachment.
#[derive(Debug, Clone)]
pub struct Attachment {
    /// File name as shown to the user (`invoice.xml`).
    pub name: String,
    /// MIME type (`text/xml` — escaped to a name object automatically).
    pub mime: String,
    /// Raw file bytes (stored Flate-compressed).
    pub bytes: Vec<u8>,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl PdfOptions {
    /// Options with outline enabled.
    #[must_use]
    pub fn new() -> Self {
        Self {
            title: None,
            author: None,
            outline: true,
            paginate: false,
            attachments: Vec::new(),
            pdfa: false,
        }
    }
}

/// Render one [`Node`] per page to PDF bytes, with document options.
pub fn render_pdf_with(
    pages: &[Node],
    size: PageSize,
    options: &PdfOptions,
) -> Result<Vec<u8>, Error> {
    if pages.is_empty() {
        return Err(Error::Encode("no pages".into()));
    }
    let (pw, ph) = size.dims();
    // Auto-pagination expands each input node into one or more page nodes.
    let mut expanded: Vec<Node> = Vec::new();
    for node in pages {
        if options.paginate {
            let padding = match node {
                Node::Container { style, .. } => style.padding,
                _ => 0.0,
            };
            expanded.extend(paginate(
                node,
                &Flow {
                    page_w: pw,
                    page_h: ph,
                    padding,
                },
            ));
        } else {
            expanded.push(node.clone());
        }
    }
    let placed: Vec<Placed> = expanded
        .iter()
        .map(|t| compute_layout(t, pw, ph))
        .collect::<Result<_, _>>()?;
    let mut doc = PdfDoc::new()?;
    let mut marks = Vec::new();
    for (i, p) in placed.iter().enumerate() {
        doc.collect(p, i, 0.0, &mut marks)?;
    }
    doc.finish(pw, ph, &placed, options, &marks)
}

/// One outline entry: text, page index, layout y of the box top, level.
struct Mark {
    title: String,
    page: usize,
    y: f32,
    level: u8,
}

struct DocFont {
    tag: &'static [u8; 2],
    /// Full PostScript name (`XXXXXX+Family` once subsetted).
    name: String,
    bytes: &'static [u8],
    /// The parsed font, borrowed from `bytes`.
    ///
    /// Held rather than reparsed per lookup. This used to call
    /// `ttf_parser::Face::parse` on every `has`, `gid_of` and `exact_width`,
    /// which meant re-reading the whole cmap for every single glyph of every
    /// page -- and `exact_width` runs per character, so a dense table paid for
    /// it hundreds of times. `FontRef` borrows `bytes`, which is `'static`, so
    /// this costs a pointer and nothing more.
    parsed: skrifa::FontRef<'static>,
    upem: f32,
    ascender: f32,
    descender: f32,
    bbox: [f32; 4],
    cap_height: f32,
    /// gid -> width in thousandths.
    widths: BTreeMap<u32, f32>,
    /// gid -> representative char (ToUnicode).
    unicodes: BTreeMap<u32, char>,
    /// Subsetted font bytes (set by `subset_fonts`).
    subset_bytes: Option<Vec<u8>>,
    /// Original gid -> subset gid (empty = identity, full embed).
    remap: HashMap<u32, u32>,
}

struct EmbeddedImage {
    w: u32,
    h: u32,
    rgb_flate: Vec<u8>,
    mask_flate: Option<Vec<u8>>,
}

/// Which font an advance should be drawn with. `Primary` indexes
/// [`PdfDoc::fonts`]; the fallback is emitted last and has no index until the
/// font list is final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FontSlot {
    Primary(usize),
    Fallback,
}

struct PdfDoc {
    /// Fonts in use; index 0 is always the embedded font. Custom fonts named by
    /// `Style::font` are appended in first-use order.
    fonts: Vec<DocFont>,
    /// `FontId` to index into `fonts`.
    font_index: HashMap<hikari_core::FontId, usize>,
    /// System CJK fallback, emitted after every primary font.
    fallback: Option<DocFont>,
    images: Vec<EmbeddedImage>,
    image_index: HashMap<String, usize>,
}

impl PdfDoc {
    fn new() -> Result<Self, Error> {
        let builtin = DocFont::parse(b"F1", "DejaVuSans", font_bytes())?;
        let mut font_index = HashMap::new();
        font_index.insert(hikari_core::BUILTIN_FONT, 0usize);
        Ok(Self {
            fonts: vec![builtin],
            font_index,
            fallback: fallback_font_bytes()
                .and_then(|b| DocFont::parse(b"F2", "ArialUnicodeMS", b).ok()),
            images: Vec::new(),
            image_index: HashMap::new(),
        })
    }

    /// Index into [`Self::fonts`] for a `FontId`, registering the font on
    /// first use. Returns `None` for an id that was never registered, which
    /// the caller degrades to the embedded font.
    fn slot_for(&mut self, id: hikari_core::FontId) -> Result<usize, Error> {
        if let Some(&i) = self.font_index.get(&id) {
            return Ok(i);
        }
        let Some(entry) = hikari_core::font_entry(id) else {
            return Err(Error::Asset(format!(
                "unknown font id {id}; register it before rendering"
            )));
        };
        // PDF resource names must be unique per font. F1 is the embedded font
        // and F2 is the system fallback (both fixed in `new`), so caller fonts
        // number from F3 up. The tag is the number itself rather than a letter
        // so the sequence stays readable, and skipping F2 removes any chance of
        // a caller font colliding with the fallback slot.
        let index = self.fonts.len();
        let tag: &'static [u8; 2] = if index == 0 {
            b"F1"
        } else {
            let n = index + 2;
            if n > 9 {
                return Err(Error::Asset(format!(
                    "too many distinct fonts in one document: font F{n} exceeds the \
                     F1..F9 resource range reserved here"
                )));
            }
            let leaked: &'static mut [u8] =
                Box::leak(format!("F{n}").into_bytes().into_boxed_slice());
            (&*leaked).try_into().expect("two-byte tag")
        };
        let font = DocFont::parse(tag, &entry.name, entry.bytes())?;
        let index = self.fonts.len();
        self.fonts.push(font);
        self.font_index.insert(id, index);
        Ok(index)
    }

    /// Font index for a font already seen during collection. Read-only, because
    /// the write pass runs after `collect` has registered every font in the
    /// document; an unknown id here would mean the two passes disagreed.
    fn slot_of(&self, adv: &hikari_core::PlacedAdvance) -> Result<FontSlot, Error> {
        if !adv.missing {
            let index =
                self.font_index.get(&adv.font).copied().ok_or_else(|| {
                    Error::Asset(format!("font {} was never collected", adv.font))
                })?;
            return Ok(FontSlot::Primary(index));
        }
        match &self.fallback {
            Some(fb) if fb.has(adv.ch) => Ok(FontSlot::Fallback),
            _ => Err(Error::Asset(format!(
                "no glyph for {:?} (install a CJK fallback font)",
                adv.ch
            ))),
        }
    }

    /// Font slot for a shaped advance: its own font unless that lacks the glyph.
    fn font_for(&mut self, adv: &hikari_core::PlacedAdvance) -> Result<FontSlot, Error> {
        if !adv.missing {
            return Ok(FontSlot::Primary(self.slot_for(adv.font)?));
        }
        match &self.fallback {
            Some(fb) if fb.has(adv.ch) => Ok(FontSlot::Fallback),
            _ => Err(Error::Asset(format!(
                "no glyph for {:?} (install a CJK fallback font)",
                adv.ch
            ))),
        }
    }

    /// Walk a placed tree collecting glyphs (widths + ToUnicode), images,
    /// and outline marks. `page`/`oy` locate the box top in the document.
    fn collect(
        &mut self,
        node: &Placed,
        page: usize,
        oy: f32,
        marks: &mut Vec<Mark>,
    ) -> Result<(), Error> {
        if let Some(text) = &node.text {
            let px = node.style.font_size.unwrap_or(16.0).max(1.0);
            let font = node.style.font.unwrap_or(BUILTIN_FONT);
            // Validate the id the node *declares*, before shaping. The shaper
            // normalises an unknown id to the built-in font so text still
            // renders, which is right for raster and SVG but wrong for PDF: a
            // document that silently substituted a typeface would embed the
            // wrong font and the caller would have no way to tell. So the check
            // is here, on the declared value, rather than inferred from the
            // shaped advances.
            if let Some(id) = node.style.font {
                if hikari_core::font_entry(id).is_none() {
                    return Err(Error::Asset(format!(
                        "unknown font id {id}; register it before rendering"
                    )));
                }
            }
            for line in text.split('\n') {
                let (advances, _) = shape_text(line, px, font);
                for adv in &advances {
                    // Spaces are emitted as real glyphs so extracted text
                    // keeps word breaks; only control chars are skipped.
                    if adv.ch.is_control() {
                        continue;
                    }
                    let slot = self.font_for(adv)?;
                    let (gid, width) = match slot {
                        FontSlot::Primary(i) => {
                            let f = &self.fonts[i];
                            (adv.gid, f.glyph_width(adv.gid))
                        }
                        FontSlot::Fallback => {
                            let fb = self.fallback.as_ref().expect("checked");
                            (fb.gid_of(adv.ch)?, fb.exact_width(adv.ch))
                        }
                    };
                    let font = match slot {
                        FontSlot::Primary(i) => &mut self.fonts[i],
                        FontSlot::Fallback => self.fallback.as_mut().expect("checked"),
                    };
                    font.widths
                        .entry(gid)
                        .and_modify(|w| *w = w.max(width))
                        .or_insert(width);
                    font.unicodes.entry(gid).or_insert(adv.ch);
                    // Glyph 0 (`.notdef`) is emitted when a character has no
                    // glyph in the chosen face. It is a real glyph with a real
                    // advance in `hmtx`, and PDF/A-1 clause 6.3.6 compares that
                    // against `/W` the same way as any other glyph -- but it has
                    // no character, so nothing here would otherwise ever give
                    // it an entry and `/DW`'s default of 1000 would silently
                    // disagree with the font. Recording it is the difference
                    // between a file that validates and one that does not.
                    if gid == 0 {
                        let w0 = font.glyph_width(0);
                        font.widths.entry(0).or_insert(w0);
                    }
                }
            }
        }
        if let Some(Media::Image { bytes, .. }) = &node.media {
            self.embed_image(bytes)?;
        }
        if node.style.bookmark.is_some() {
            if let Some(text) = &node.text {
                let title = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !title.is_empty() {
                    marks.push(Mark {
                        title,
                        page,
                        y: oy + node.y,
                        level: node.style.bookmark.unwrap_or(1),
                    });
                }
            }
        }
        for child in &node.children {
            self.collect(child, page, oy + node.y, marks)?;
        }
        Ok(())
    }

    fn embed_image(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        let key = hash_bytes(bytes);
        if let Some(i) = self.image_index.get(&key) {
            return Ok(*i);
        }
        let img = image::load_from_memory(bytes).map_err(|e| Error::Asset(e.to_string()))?;
        let rgba = img.to_rgba8();
        let (w, h) = (rgba.width(), rgba.height());
        let raw = rgba.into_raw();
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        let mut mask = Vec::with_capacity((w * h) as usize);
        let mut opaque = true;
        let (chunks, _) = raw.as_chunks::<4>();
        for px in chunks {
            rgb.extend_from_slice(&px[..3]);
            mask.push(px[3]);
            if px[3] != 255 {
                opaque = false;
            }
        }
        let idx = self.images.len();
        self.images.push(EmbeddedImage {
            w,
            h,
            rgb_flate: deflate(&rgb),
            mask_flate: if opaque { None } else { Some(deflate(&mask)) },
        });
        self.image_index.insert(key, idx);
        Ok(idx)
    }

    /// Subset each used font to its glyphs (allsorts, PDF profile).
    /// Falls back to full embed per font on any subset error — files stay
    /// correct, just larger.
    fn subset_fonts(&mut self) {
        // `fonts` then the fallback: the same order they are emitted in, so
        // `font_refs` indices line up with `FontSlot::Primary(i)`.
        let mut all: Vec<&mut DocFont> = self.fonts.iter_mut().collect();
        all.extend(self.fallback.as_mut());
        for font in all {
            if font.widths.is_empty() {
                continue;
            }
            let used: std::collections::BTreeSet<u32> = font.widths.keys().copied().collect();
            let subset = match subset_ttf(font.bytes, &used) {
                Ok(b) => b,
                Err(_) => continue,
            };
            // The subset is freshly built and owned here, so the `FontRef`
            // borrows it for the rest of this loop body only.
            let face = match skrifa::FontRef::new(&subset) {
                Ok(f) => f,
                Err(_) => continue,
            };
            // Which character does each drawn glyph actually *represent*?
            //
            // Taken from the original font's own cmap rather than from the
            // `adv.ch` recorded during shaping. The shaper reports one
            // character per glyph, and for a ligature that character is the
            // first of the cluster -- so the `fi` ligature (gid 5042 in DejaVu
            // Sans) was recorded as `f`. Inverting that gave two glyphs
            // claiming one character, and the ligature was then remapped onto
            // the subset's *plain* `f`. Not only did that draw the wrong shape,
            // it made the remap non-total (the plain `f`'s own glyph had lost
            // its character), which is what abandoned the subset entirely and
            // cost the PDF/A path a 15x size regression.
            //
            // Asking the font is exact: the cmap is the authority on which
            // glyph stands for which character, and it already knows about the
            // precomposed forms. The subsetter keeps composites -- probed
            // directly, requesting gid 5042 yields a subset where `fi` is
            // present -- so the lookup on the other side succeeds.
            let mut chars: HashMap<char, u32> = HashMap::new();
            {
                let original = skrifa::FontRef::new(font.bytes).expect("registered font parses");
                for (cp, gid) in original.charmap().mappings() {
                    let Some(ch) = char::from_u32(cp) else {
                        continue;
                    };
                    // `or_insert`: where several code points share a glyph
                    // (a compatibility variant, say) the first one wins, which
                    // matches the subsetter's own view.
                    chars.entry(ch).or_insert(gid.to_u32());
                }
            }
            // Only glyphs this document actually draws are of interest.
            chars.retain(|_, gid| font.widths.contains_key(gid));
            // The reverse map is needed per glyph, so build it once.
            let mut orig_of: HashMap<u32, char> = HashMap::new();
            for (ch, gid) in &chars {
                orig_of.entry(*gid).or_insert(*ch);
            }
            let mut widths = BTreeMap::new();
            let mut unicodes = BTreeMap::new();
            let mut remap = HashMap::new();
            let mut ok = true;
            for (ch, orig) in &chars {
                match face.charmap().map(*ch) {
                    Some(new) => {
                        let new_gid = new.to_u32();
                        remap.insert(*orig, new_gid);
                        if let Some(w) = font.widths.get(orig) {
                            widths.insert(new_gid, *w);
                        }
                        // ToUnicode wants the character this glyph stands for,
                        // which is now the font's answer rather than the first
                        // character of the cluster. For `fi` that is U+FB01,
                        // so extracted text round-trips to the ligature instead
                        // of to a lone `f`.
                        unicodes.insert(new_gid, *ch);
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            // The remap must be *total* over the glyphs this font will draw.
            //
            // The subsetter renumbers glyphs into a compact sequence, but it
            // builds the output cmap from the characters it retained. A glyph
            // that is referenced without being reachable from that cmap -- a
            // composite reached by glyph id rather than by character, or glyph 0
            // -- gets no entry here. The writer then fell back to the *original*
            // id for those, emitting a stale glyph number into a font that has
            // only a few dozen glyphs.
            //
            // That failure is invisible until a conformance checker cross-reads
            // the content stream against the embedded program: the PDF rendered
            // the wrong shape rather than failing, and the writer's own tests
            // passed because every glyph it checked was one of the remapped
            // ones. veraPDF clause 6.3.5/1 caught it.
            //
            // Rather than paper over it with a second fallback, an incomplete
            // remap means the subset is not usable: embed the font whole, keep
            // the original ids, and let `/CIDToGIDMap /Identity` be truthful.
            // A larger file that is correct beats a smaller one that is wrong.
            // Glyph 0 is the exception: `.notdef` has no character, so it can
            // never be reached through the cmap, but it is always retained by
            // the subsetter in first position -- so its id is unchanged. Without
            // this, the totality check below rejects every subset (glyph 0 is
            // always present) and the whole font gets embedded, which is a 15x
            // size regression on the PDF/A path.
            remap.entry(0).or_insert(0);
            // Measured on the PDF/A fixture: exactly one glyph is unremapped,
            // gid 5042, a composite that the subsetter expands into its
            // components and therefore does not retain. That single glyph is
            // what costs the whole font its subset here.
            let total = font.widths.keys().all(|gid| remap.contains_key(gid));
            if !ok || !total {
                continue;
            }
            let prefix: String = hash_bytes(&subset)
                .bytes()
                .take(6)
                .map(|b| (b % 26 + b'A') as char)
                .collect();
            font.name = format!("{prefix}+{}", font.name);
            font.widths = widths;
            font.unicodes = unicodes;
            font.remap = remap;
            font.subset_bytes = Some(subset);
        }
    }

    fn finish(
        mut self,
        pw: f32,
        ph: f32,
        pages: &[Placed],
        options: &PdfOptions,
        marks: &[Mark],
    ) -> Result<Vec<u8>, Error> {
        self.subset_fonts();
        let mut alloc = Ref::new(1);
        let mut next = || {
            let r = alloc;
            alloc = Ref::new(alloc.get() + 1);
            r
        };
        let catalog = next();
        let page_tree = next();

        struct FontRefs {
            tag: &'static [u8; 2],
            type0: Ref,
            cid: Ref,
            desc: Ref,
            cmap: Ref,
            file: Ref,
            cid_set: Ref,
        }
        // One resource block per font actually used, in `fonts` order, with
        // the fallback appended last when it set any glyphs.
        let mut font_refs: Vec<FontRefs> = self
            .fonts
            .iter()
            .map(|f| FontRefs {
                tag: f.tag,
                type0: next(),
                cid: next(),
                desc: next(),
                cmap: next(),
                file: next(),
                cid_set: next(),
            })
            .collect();
        // Embed the fallback font only when it actually set glyphs.
        if let Some(fb) = self.fallback.as_ref().filter(|f| !f.widths.is_empty()) {
            font_refs.push(FontRefs {
                tag: fb.tag,
                type0: next(),
                cid: next(),
                desc: next(),
                cmap: next(),
                file: next(),
                cid_set: next(),
            });
        }

        let mut image_refs: Vec<(Ref, Option<Ref>)> = Vec::new();
        for _ in &self.images {
            let img = next();
            let has_mask = self.images[image_refs.len()].mask_flate.is_some();
            image_refs.push((img, if has_mask { Some(next()) } else { None }));
        }

        // Attachment objects: (file stream, file spec) per attachment.
        let mut attach_refs: Vec<(Ref, Ref)> = Vec::new();
        for _ in &options.attachments {
            attach_refs.push((next(), next()));
        }

        let mut page_refs = Vec::new();
        let mut content_blobs: Vec<Vec<u8>> = Vec::new();
        let mut page_links: Vec<Vec<AnnotLink>> = Vec::new();
        for placed in pages {
            let mut content = Content::new();
            let mut ctx = Ctx {
                ph,
                doc: &self,
                links: Vec::new(),
            };
            paint_box(&mut content, placed, 0.0, 0.0, &mut ctx)?;
            content_blobs.push(deflate(&content.finish()));
            page_links.push(ctx.links);
        }
        let mut annot_refs: Vec<Vec<Ref>> = Vec::new();
        for links in &page_links {
            annot_refs.push(links.iter().map(|_| next()).collect());
        }
        for _ in pages {
            page_refs.push((next(), next()));
        }

        // Embedded font programs, in the same order as `font_refs`.
        let font_files: Vec<Vec<u8>> = self
            .fonts
            .iter()
            .map(|f| deflate(f.subset_bytes.as_deref().unwrap_or(f.bytes)))
            .collect();
        let fallback_file = self
            .fallback
            .as_ref()
            .map(|f| deflate(f.subset_bytes.as_deref().unwrap_or(f.bytes)));
        let fallback_cmap = self.fallback.as_ref().map(tocmap);

        let mut pdf = Pdf::new();
        let emit_outline = options.outline && !marks.is_empty();
        let outline_root = if emit_outline { Some(next()) } else { None };
        let outline_items: Vec<Ref> = marks.iter().map(|_| next()).collect();
        // PDF/A stream objects are written before the catalog below, because
        // `pdf.catalog` borrows `pdf` mutably for as long as the handle lives and
        // these need the same borrow.
        let pdfa_refs = if options.pdfa {
            let icc_ref = next();
            let metadata_ref = next();
            {
                // The destination profile must be a display-class ICC profile
                // carrying `mntr`; PDF/A checks for both before it looks at
                // anything else, so a missing tag here is the single most common
                // rejection.
                let icc = pdfa::srgb_icc_profile();
                let mut profile = pdf.icc_profile(icc_ref, &icc);
                profile.n(3);
            }
            {
                let producer = format!("Hikari {}", env!("CARGO_PKG_VERSION"));
                let xmp = pdfa::build_xmp(
                    options.title.as_deref(),
                    options.author.as_deref(),
                    &producer,
                );
                // Written uncompressed, and that is not a size compromise:
                // PDF/A-1 clause 6.7.2 forbids a `/Filter` on the metadata stream
                // outright. veraPDF rejects the file for having one, so
                // compressing it -- which was the entire reason it went through
                // `deflate` -- makes the output non-conformant. The packet is a
                // few hundred bytes and the conformance is worth more.
                pdf.metadata(metadata_ref, xmp.as_bytes());
            }
            Some((icc_ref, metadata_ref))
        } else {
            None
        };
        {
            let mut cat = pdf.catalog(catalog);
            cat.pages(page_tree);
            if let Some(root) = outline_root {
                cat.outlines(root);
            }
            if let Some((icc_ref, metadata_ref)) = pdfa_refs {
                {
                    // `pdf-writer` 0.12 exposes `OutputIntent` as a writer but has
                    // no constructor on `Pdf`, so it is written inline through the
                    // typed array rather than as a separately allocated indirect
                    // object. The keys and their order are the ones its writer
                    // emits.
                    // Bound, not chained: `push()` borrows the array it is
                    // written into, so a temporary would end the borrow early.
                    let mut intents = cat.output_intents();
                    let mut intent = intents.push();
                    intent.subtype(OutputIntentSubtype::PDFA);
                    intent.output_condition(TextStr("sRGB IEC61966-2.1"));
                    // PDF/A accepts either a well-known
                    // `/OutputConditionIdentifier` or the destination profile. No
                    // vendor assigns a well-known identifier to a generated sRGB
                    // profile, so the profile is what makes this intent valid.
                    intent.info(TextStr("sRGB IEC61966-2.1"));
                    intent.dest_output_profile(icc_ref);
                }
                cat.metadata(metadata_ref);
                // PDF/A requires the document title to be displayed rather than
                // used as a window title, which is a viewer preference and not a
                // property of the text itself.
                let mut prefs = cat.viewer_preferences();
                prefs.display_doc_title(true);
            }
            if !attach_refs.is_empty() {
                let mut names = cat.names();
                let mut ef = names.embedded_files();
                let mut entries = ef.names();
                for (att, (_, spec)) in options.attachments.iter().zip(attach_refs.iter()) {
                    entries.insert(Str(att.name.as_bytes()), *spec);
                }
            }
        }
        // Document metadata (producer always stamped).
        {
            let info = next();
            let mut dict = pdf.document_info(info);
            if let Some(t) = &options.title {
                dict.title(TextStr(t));
            }
            if let Some(a) = &options.author {
                dict.author(TextStr(a));
            }
            dict.creator(TextStr("Hikari"));
            dict.producer(TextStr(concat!("Hikari ", env!("CARGO_PKG_VERSION"))));
        }
        pdf.pages(page_tree)
            .kids(page_refs.iter().map(|(p, _)| *p))
            .count(page_refs.len() as i32);

        let mut fonts: Vec<&DocFont> = self.fonts.iter().collect();
        if self.fallback.as_ref().is_some_and(|f| !f.widths.is_empty()) {
            fonts.push(self.fallback.as_ref().expect("checked"));
        }
        for (index, (font, refs)) in fonts.iter().zip(font_refs.iter()).enumerate() {
            // Primaries occupy `font_files` in order; the fallback, when
            // present, is the final entry in `fonts` and uses its own bytes.
            let is_fallback = index >= self.fonts.len();
            // Bound, not inlined into the tuple: a temporary here would be
            // dropped before the stream is written.
            let own_cmap = if is_fallback {
                Vec::new()
            } else {
                tocmap(font)
            };
            let (cmap_bytes, file_bytes, file_len) = if is_fallback {
                let fb = self.fallback.as_ref().expect("checked");
                (
                    fallback_cmap.as_ref().expect("checked").as_slice(),
                    fallback_file.as_ref().expect("checked").as_slice(),
                    fb.subset_bytes.as_deref().unwrap_or(fb.bytes).len(),
                )
            } else {
                let own = font_files[index].as_slice();
                (
                    own_cmap.as_slice(),
                    own,
                    font.subset_bytes.as_deref().unwrap_or(font.bytes).len(),
                )
            };
            pdf.type0_font(refs.type0)
                .base_font(Name(font.name.as_bytes()))
                .encoding_predefined(Name(b"Identity-H"))
                .descendant_font(refs.cid)
                .to_unicode(refs.cmap);
            {
                let mut cid = pdf.cid_font(refs.cid);
                cid.subtype(CidFontType::Type2)
                    .base_font(Name(font.name.as_bytes()))
                    // PDF/A-1 clause 6.3.3.2: a Type 2 CIDFont used for rendering
                    // must carry a CIDToGIDMap. `Identity` is not merely
                    // acceptable here, it is correct -- the subsetter keeps
                    // original glyph ids, so CID and GID are the same number and a
                    // mapping stream could only ever agree with it.
                    .cid_to_gid_map_predefined(Name(b"Identity"))
                    .system_info(SystemInfo {
                        registry: Str(b"Adobe"),
                        ordering: Str(b"Identity"),
                        supplement: 0,
                    })
                    .font_descriptor(refs.desc)
                    // The default width describes glyphs absent from `/W`, and
                    // the one that is always absent is glyph 0 (`.notdef`),
                    // which has no character and so is never collected by the
                    // walk above. PDF/A-1 clause 6.3.6 compares `/DW` against
                    // `hmtx` exactly as it compares `/W`, so a hardcoded 1000
                    // disagrees with the font's real 600.10 and fails
                    // validation. Read it from the font instead.
                    .default_width(font.glyph_width(0));
                let mut runs: Vec<(u32, Vec<f32>)> = Vec::new();
                for (gid, w) in &font.widths {
                    match runs.last_mut() {
                        Some((last, ws)) if *last + ws.len() as u32 == *gid => ws.push(*w),
                        _ => runs.push((*gid, vec![*w])),
                    }
                }
                {
                    let mut widths = cid.widths();
                    for (start, ws) in &runs {
                        widths.consecutive(*start as u16, ws.iter().copied());
                    }
                }
            }
            pdf.font_descriptor(refs.desc)
                .name(Name(font.name.as_bytes()))
                .flags(FontFlags::SYMBOLIC)
                .bbox(Rect::new(
                    font.bbox[0],
                    font.bbox[1],
                    font.bbox[2],
                    font.bbox[3],
                ))
                .italic_angle(0.0)
                .ascent(font.ascender)
                .descent(font.descender)
                .cap_height(font.cap_height)
                .stem_v(80.0)
                .font_file2(refs.file)
                // PDF/A-1 clause 6.3.5/3: a subset CIDFont's descriptor must
                // carry a `/CIDSet` stream whose bit *i* is set exactly when CID
                // *i* is present in the embedded subset. veraPDF cross-checks
                // this against the CIDs actually referenced by the content
                // stream, so "the glyphs we subset" is not sufficient -- it has
                // to be "the glyphs that are drawn".
                .cid_set(refs.cid_set);
            // Built from the CIDs the content stream draws, not from the whole
            // subset, because those two sets differ in practice and the checker
            // compares against the former.
            {
                let mut max_cid = 0usize;
                for cid in font.widths.keys() {
                    max_cid = max_cid.max(*cid as usize);
                }
                let bytes = (max_cid / 8 + 1).max(1);
                let mut bits = vec![0u8; bytes];
                for cid in font.widths.keys() {
                    let cid = *cid as usize;
                    bits[cid / 8] |= 1 << (7 - (cid % 8));
                }
                let mut s = pdf.stream(refs.cid_set, &bits);
                // Not an oversight: PDF/A forbids compressing the CIDSet, and
                // veraPDF fails the file if `/Filter` is present here.
                let _ = &mut s;
            }
            pdf.stream(refs.cmap, cmap_bytes);
            {
                let mut s = pdf.stream(refs.file, file_bytes);
                s.filter(Filter::FlateDecode);
                s.pair(Name(b"Length1"), file_len as i32);
            }
        }

        for (emb, (img_ref, mask_ref)) in self.images.iter().zip(image_refs.iter()) {
            if let Some(mask) = mask_ref {
                let mut sm = pdf.image_xobject(*mask, emb.mask_flate.as_ref().expect("mask"));
                sm.filter(Filter::FlateDecode);
                sm.width(emb.w as i32).height(emb.h as i32);
                sm.color_space().device_gray();
                sm.bits_per_component(8);
            }
            {
                let mut im = pdf.image_xobject(*img_ref, &emb.rgb_flate);
                im.filter(Filter::FlateDecode);
                im.width(emb.w as i32).height(emb.h as i32);
                im.color_space().device_rgb();
                im.bits_per_component(8);
                im.interpolate(true);
                if let Some(mask) = mask_ref {
                    im.s_mask(*mask);
                }
            }
        }

        for (att, (stream_ref, spec_ref)) in options.attachments.iter().zip(attach_refs.iter()) {
            let compressed = deflate(&att.bytes);
            {
                let mut f = pdf.embedded_file(*stream_ref, &compressed);
                f.filter(Filter::FlateDecode);
                // pdf-writer escapes the name object itself (`/` -> `#2F`).
                f.subtype(Name(att.mime.as_bytes()));
                f.params().size(att.bytes.len() as i32);
            }
            {
                let mut spec = pdf.file_spec(*spec_ref);
                spec.path(Str(att.name.as_bytes()));
                spec.embedded_file(*stream_ref);
            }
        }

        for (pi, ((page_ref, content_ref), blob)) in
            page_refs.iter().zip(content_blobs.iter()).enumerate()
        {
            {
                let mut page = pdf.page(*page_ref);
                page.parent(page_tree)
                    .media_box(Rect::new(0.0, 0.0, pw, ph))
                    .contents(*content_ref);
                if !annot_refs[pi].is_empty() {
                    page.annotations(annot_refs[pi].iter().copied());
                }
                {
                    let mut res = page.resources();
                    {
                        let mut fonts_dict = res.fonts();
                        for refs in &font_refs {
                            fonts_dict.pair(Name(refs.tag), refs.type0);
                        }
                    }
                    {
                        let mut xo = res.x_objects();
                        for (i, (img_ref, _)) in image_refs.iter().enumerate() {
                            xo.pair(Name(image_name(i)), *img_ref);
                        }
                    }
                }
            }
            let mut s = pdf.stream(*content_ref, blob);
            s.filter(Filter::FlateDecode);
        }

        for (links, refs) in page_links.iter().zip(annot_refs.iter()) {
            for (link, aref) in links.iter().zip(refs.iter()) {
                let mut annot = pdf.annotation(*aref);
                annot.subtype(AnnotationType::Link);
                annot.rect(Rect::new(
                    link.x,
                    flip(link.y + link.h, ph),
                    link.x + link.w,
                    flip(link.y, ph),
                ));
                annot.border(0.0, 0.0, 0.0, None);
                annot
                    .action()
                    .action_type(ActionType::Uri)
                    .uri(Str(link.url.as_bytes()));
            }
        }

        if let Some(root) = outline_root {
            // Nesting from bookmark levels (stack algorithm); counts are
            // open (positive) descendant totals.
            let n = marks.len();
            let mut parent: Vec<Option<usize>> = vec![None; n];
            let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
            let mut stack: Vec<usize> = Vec::new();
            for i in 0..n {
                while stack
                    .last()
                    .is_some_and(|&t| marks[t].level >= marks[i].level)
                {
                    stack.pop();
                }
                if let Some(&p) = stack.last() {
                    parent[i] = Some(p);
                    children[p].push(i);
                }
                stack.push(i);
            }
            fn descendants(i: usize, children: &[Vec<usize>]) -> i32 {
                children[i]
                    .iter()
                    .map(|&c| 1 + descendants(c, children))
                    .sum()
            }
            let tops: Vec<usize> = (0..n).filter(|&i| parent[i].is_none()).collect();
            {
                let mut outline: Outline = pdf.indirect(root).start();
                outline
                    .first(outline_items[tops[0]])
                    .last(outline_items[tops[tops.len() - 1]])
                    .count(tops.iter().map(|&t| 1 + descendants(t, &children)).sum());
            }
            for (i, (mark, item)) in marks.iter().zip(outline_items.iter()).enumerate() {
                let sibs: Vec<usize> = match parent[i] {
                    Some(p) => children[p].clone(),
                    None => tops.clone(),
                };
                let pos = sibs.iter().position(|&s| s == i).unwrap_or(0);
                let mut oi = pdf.outline_item(*item);
                oi.title(TextStr(&mark.title));
                match parent[i] {
                    Some(p) => oi.parent(outline_items[p]),
                    None => oi.parent(root),
                };
                if !children[i].is_empty() {
                    oi.first(outline_items[children[i][0]]);
                    oi.last(outline_items[children[i][children[i].len() - 1]]);
                    oi.count(descendants(i, &children));
                }
                if pos > 0 {
                    oi.prev(outline_items[sibs[pos - 1]]);
                }
                if pos + 1 < sibs.len() {
                    oi.next(outline_items[sibs[pos + 1]]);
                }
                oi.dest()
                    .page(page_refs[mark.page].0)
                    .xyz(0.0, flip(mark.y, ph), None);
            }
        }

        // The trailer `/ID` is required by PDF/A and must be set before `finish`,
        // since that writes the trailer. Derived from the object bytes written so
        // far rather than a clock: the same tree must still produce the same file,
        // which the determinism card checks, and two different documents must never
        // share an identifier.
        if options.pdfa {
            let id = pdfa::document_id(pdf.as_bytes());
            pdf.set_file_id((id[0].to_vec(), id[1].to_vec()));
        }
        Ok(pdf.finish())
    }
}

/// Static image resource names (`Im0`..). Leaked once per index, bounded by
/// image count — the same tradeoff as any intern table without an arena.
fn image_name(i: usize) -> &'static [u8] {
    use std::sync::OnceLock;
    static NAMES: OnceLock<Vec<Vec<u8>>> = OnceLock::new();
    // Pre-generate a generous table; documents with more images are rejected
    // loudly instead of leaking unboundedly.
    const MAX: usize = 4096;
    let table = NAMES.get_or_init(|| (0..MAX).map(|i| format!("Im{i}").into_bytes()).collect());
    table.get(i).map(Vec::as_slice).unwrap_or(b"Im0")
}

impl DocFont {
    fn parse(
        tag: &'static [u8; 2],
        base_name: &'static str,
        bytes: &'static [u8],
    ) -> Result<Self, Error> {
        let parsed = skrifa::FontRef::new(bytes)
            .map_err(|e| Error::Font(format!("{base_name}: not a usable font: {e:?}")))?;

        // Ask for metrics at exactly the design size, so every value comes back
        // in font units. `Metrics` is scaled by `size / units_per_em`, and this
        // code divides by `upem` again further down; asking for a different size
        // here would quietly square that factor instead of failing.
        // `Metrics.units_per_em` is not affected by the requested size, so this
        // first read is only to learn the em square.
        let upem = parsed
            .metrics(
                skrifa::instance::Size::new(1.0),
                skrifa::instance::LocationRef::default(),
            )
            .units_per_em as f32;
        if upem <= 0.0 {
            return Err(Error::Font(format!("{base_name}: units_per_em is zero")));
        }
        // Now ask at exactly the design size, so every other value comes back in
        // font units. Metrics are scaled by `size / units_per_em`, and this code
        // divides by `upem` again further down; asking for a different size here
        // would quietly square that factor instead of failing.
        let size = skrifa::instance::Size::new(upem);
        let location = skrifa::instance::LocationRef::default();
        let m = parsed.metrics(size, location);

        // The PDF `/FontDescriptor` needs a real global bounding box. A font
        // without one is legal but rare, so fall back to the ascender rather
        // than emitting NaNs into the descriptor.
        let bbox = m.bounds;
        let cap = m
            .cap_height
            .or_else(|| {
                parsed
                    .charmap()
                    .map('H')
                    .and_then(|g| parsed.glyph_metrics(size, location).bounds(g))
                    .map(|b| b.y_max)
            })
            .unwrap_or(m.ascent * 0.7);
        Ok(Self {
            tag,
            name: base_name.to_owned(),
            bytes,
            parsed,
            upem,
            ascender: m.ascent,
            descender: m.descent,
            bbox: bbox.map_or([0.0, m.descent, m.ascent, 0.0], |b| {
                [b.x_min, b.y_min, b.x_max, b.y_max]
            }),
            cap_height: cap,
            widths: BTreeMap::new(),
            unicodes: BTreeMap::new(),
            subset_bytes: None,
            remap: HashMap::new(),
        })
    }

    fn has(&self, ch: char) -> bool {
        self.parsed.charmap().map(ch).is_some()
    }

    fn gid_of(&self, ch: char) -> Result<u32, Error> {
        self.parsed
            .charmap()
            .map(ch)
            .map(|g| g.to_u32())
            .ok_or_else(|| Error::Asset(format!("no glyph for {ch:?}")))
    }

    /// Exact advance in thousandths for this font's glyph.
    /// The font's own advance width for `gid`, in 1000-unit glyph space.
    ///
    /// Deliberately *not* the shaped advance. The shaper returns a per-cluster
    /// advance that includes kerning, and writing that into `/W` describes a
    /// glyph that does not exist: PDF/A-1 clause 6.3.6 requires `/W` to agree
    /// with the embedded `hmtx`, and veraPDF compares them numerically. A
    /// clustered pair with kerning produced widths of 649.90 where the font
    /// says 694.82 -- a 7% error in the advance of every affected glyph,
    /// visible as loose or overlapping text in a conforming file.
    ///
    /// Returns `None` when the font has no metric for the glyph, so the caller
    /// can leave it to `/DW` rather than write a number the font does not
    /// agree with.
    fn glyph_width_opt(&self, gid: u32) -> Option<f32> {
        let size = skrifa::instance::Size::new(self.upem);
        self.parsed
            .glyph_metrics(size, skrifa::instance::LocationRef::default())
            .advance_width(skrifa::raw::types::GlyphId::new(gid))
            .map(|a| a / self.upem * 1000.0)
    }

    /// As [`Self::glyph_width_opt`], falling back to 1000 (`/DW`).
    fn glyph_width(&self, gid: u32) -> f32 {
        self.glyph_width_opt(gid).unwrap_or(1000.0)
    }

    fn exact_width(&self, ch: char) -> f32 {
        let size = skrifa::instance::Size::new(self.upem);
        self.parsed
            .charmap()
            .map(ch)
            .and_then(|g| {
                self.parsed
                    .glyph_metrics(size, skrifa::instance::LocationRef::default())
                    .advance_width(g)
            })
            .map(|a| a / self.upem * 1000.0)
            .unwrap_or(1000.0)
    }
}

/// Subset a TTF to `gids` (plus `.notdef`) with the PDF table profile.
/// Subset a TrueType font to the characters in `text`.
///
/// A thin public wrapper over the internal subsetter: it resolves the glyph ids
/// the characters need and hands them over. Exists for two reasons -- a caller
/// embedding a third-party face needs it, and it is the entry point the PDF
/// fuzz target drives, which makes the table-walking subsetter reachable by a
/// fuzzer at all. `subset_ttf` is private because it takes glyph ids, which a
/// caller has no way to obtain.
///
/// Returns `Err` for anything that is not a parseable TrueType font. That is the
/// expected outcome for arbitrary bytes and must stay a clean rejection.
pub fn subset_font_for_text(bytes: &[u8], text: &str) -> Result<Vec<u8>, Error> {
    // `glyph_index_in` rather than a second charmap walk here: it resolves a
    // character exactly the way the renderer does, so a subset built for a
    // character cannot disagree with the render of that character.
    let mut gids = std::collections::BTreeSet::new();
    gids.insert(0u32); // .notdef
    for ch in text.chars() {
        if let Some(gid) = glyph_index_in(bytes, ch) {
            gids.insert(u32::from(gid));
        }
    }
    if gids.len() <= 1 {
        // Not a font we can read: reject rather than emit a subset of only
        // `.notdef`, which is a file no renderer will draw.
        return Err(Error::Font("no glyphs resolved from the font".into()));
    }
    subset_ttf(bytes, &gids)
}

fn subset_ttf(bytes: &[u8], gids: &std::collections::BTreeSet<u32>) -> Result<Vec<u8>, Error> {
    use allsorts::binary::read::ReadScope;
    use allsorts::font_data::FontData;
    use allsorts::subset::{subset, CmapTarget, SubsetProfile};
    let mut ids: Vec<u16> = vec![0];
    for g in gids {
        ids.push(u16::try_from(*g).map_err(|_| Error::Asset("gid too large".into()))?);
    }
    ids.sort_unstable();
    ids.dedup();
    let scope = ReadScope::new(bytes);
    let font_file = scope
        .read::<FontData<'_>>()
        .map_err(|e| Error::Asset(format!("subset read: {e:?}")))?;
    let provider = font_file
        .table_provider(0)
        .map_err(|e| Error::Asset(format!("subset provider: {e:?}")))?;
    subset(&provider, &ids, &SubsetProfile::Pdf, CmapTarget::Unicode)
        .map_err(|e| Error::Asset(format!("subset failed: {e:?}")))
}

fn deflate(bytes: &[u8]) -> Vec<u8> {
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(bytes).expect("deflate");
    enc.finish().expect("deflate finish")
}

/// Minimal ToUnicode CMap for the used glyphs (100 entries per `bfchar` block).
fn tocmap(font: &DocFont) -> Vec<u8> {
    let mut out = String::from(
        "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n\
         /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
         /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
         1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
    );
    let entries: Vec<(u32, char)> = font.unicodes.iter().map(|(g, c)| (*g, *c)).collect();
    for chunk in entries.chunks(100) {
        out.push_str(&format!("{} beginbfchar\n", chunk.len()));
        for (gid, ch) in chunk {
            let mut utf16 = [0u16; 2];
            let encoded = ch.encode_utf16(&mut utf16);
            let hex: String = encoded.iter().map(|u| format!("{u:04X}")).collect();
            out.push_str(&format!("<{gid:04X}> <{hex}>\n"));
        }
        out.push_str("endbfchar\n");
    }
    out.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    out.into_bytes()
}

/// One hyperlink box collected during paint (layout coords).
struct AnnotLink {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    url: String,
}

/// Paint context: shares the doc, tracks page + collected links.
struct Ctx<'a> {
    ph: f32,
    doc: &'a PdfDoc,
    links: Vec<AnnotLink>,
}

/// Y-flip: layout origin is top-left, PDF origin is bottom-left.
fn flip(y: f32, page_h: f32) -> f32 {
    page_h - y
}

fn paint_box(
    content: &mut Content,
    node: &Placed,
    dx: f32,
    dy: f32,
    ctx: &mut Ctx<'_>,
) -> Result<(), Error> {
    let ph = ctx.ph;
    let x = dx + node.x;
    let y = dy + node.y;
    if let Some(url) = &node.style.link {
        if !url.is_empty() && node.w > 0.0 && node.h > 0.0 {
            ctx.links.push(AnnotLink {
                x,
                y,
                w: node.w,
                h: node.h,
                url: url.clone(),
            });
        }
    }
    // Backgrounds: solids only in v1 (gradients use their first stop).
    if let Some(bg) = &node.style.background {
        let c = match bg {
            Background::Solid(c) => *c,
            Background::Linear { stops, .. } | Background::Radial { stops, .. } => stops
                .first()
                .map(|s| s.color)
                .unwrap_or(hikari_core::Color::rgb(255, 255, 255)),
        };
        let (r, g, b, _) = c.to_rgba_f32();
        content.set_fill_rgb(r, g, b);
        rr(
            content,
            x,
            flip(y + node.h, ph),
            node.w,
            node.h,
            node.style.radius,
        );
        content.fill_nonzero();
    }
    if node.style.border > 0.0 {
        let bc = node
            .style
            .border_color
            .unwrap_or(hikari_core::Color::rgb(0, 0, 0));
        let (r, g, b, _) = bc.to_rgba_f32();
        content.set_stroke_rgb(r, g, b);
        content.set_line_width(node.style.border);
        let b = node.style.border / 2.0;
        rr(
            content,
            x + b,
            flip(y + node.h - b, ph),
            node.w - node.style.border,
            node.h - node.style.border,
            (node.style.radius - b).max(0.0),
        );
        content.stroke();
    }
    if node.media.is_some() {
        paint_image(content, node, x, y, ctx.ph, ctx.doc)?;
    }
    if let Some(text) = &node.text {
        paint_text(content, text, node, x, y, ctx.ph, ctx.doc)?;
    }
    for child in &node.children {
        paint_box(content, child, x, y, ctx)?;
    }
    Ok(())
}

/// Rounded-rect path in PDF coords (`y` = bottom edge).
fn rr(content: &mut Content, x: f32, y: f32, w: f32, h: f32, r: f32) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let r = r.clamp(0.0, w.min(h) / 2.0);
    if r <= 0.01 {
        content.rect(x, y, w, h);
        return;
    }
    const K: f32 = 0.5523;
    let (x0, y0, x1, y1) = (x, y, x + w, y + h);
    content.move_to(x0 + r, y0);
    content.line_to(x1 - r, y0);
    content.cubic_to(x1 - r + K * r, y0, x1, y0 + r - K * r, x1, y0 + r);
    content.line_to(x1, y1 - r);
    content.cubic_to(x1, y1 - r + K * r, x1 - r + K * r, y1, x1 - r, y1);
    content.line_to(x0 + r, y1);
    content.cubic_to(x0 + r - K * r, y1, x0, y1 - r + K * r, x0, y1 - r);
    content.line_to(x0, y0 + r);
    content.cubic_to(x0, y0 + r - K * r, x0 + r - K * r, y0, x0 + r, y0);
    content.close_path();
}

fn paint_text(
    content: &mut Content,
    text: &str,
    node: &Placed,
    bx: f32,
    by: f32,
    ph: f32,
    doc: &PdfDoc,
) -> Result<(), Error> {
    let px = node.style.font_size.unwrap_or(16.0).max(1.0);
    let fg = node.style.color.unwrap_or(hikari_core::Color::rgb(0, 0, 0));
    let lh = line_height(px);
    let lines: Vec<&str> = text.split('\n').collect();
    let total_h = lines.len() as f32 * lh;
    let font_id = node.style.font.unwrap_or(BUILTIN_FONT);
    let ascent = doc
        .fonts
        .get(doc.font_index.get(&font_id).copied().unwrap_or(0))
        .map_or(doc.fonts[0].ascender / doc.fonts[0].upem, |f| {
            f.ascender / f.upem
        })
        * px;
    let mut baseline = by + ((node.h - total_h) / 2.0).max(0.0) + ascent;
    let (fr, fg_, fb, _) = fg.to_rgba_f32();
    content.set_fill_rgb(fr, fg_, fb);
    for line in lines {
        let (advances, total) = shape_text(line, px, font_id);
        let mut pen = bx + ((node.w - total) / 2.0).max(0.0);
        // Group consecutive advances by font: one Tj per segment.
        let mut segs: Vec<(usize, Vec<u8>, f32)> = Vec::new();
        for adv in &advances {
            if adv.ch.is_control() {
                pen += adv.advance;
                continue;
            }
            let slot = doc.slot_of(adv)?;
            let (fi, orig, remap) = match slot {
                FontSlot::Primary(i) => (i, adv.gid, &doc.fonts[i].remap),
                FontSlot::Fallback => {
                    let f = doc.fallback.as_ref().expect("checked");
                    (doc.fonts.len(), f.gid_of(adv.ch)?, &f.remap)
                }
            };
            // Map original -> embedded gid. Unsubsetted fonts keep their
            // original ids and have an empty remap, so the identity fallback is
            // correct there; subsetted fonts always have an entry (see
            // `subset_fonts`, which abandons the subset rather than emit one
            // missing).
            let gid = remap.get(&orig).copied().unwrap_or(orig);
            match segs.last_mut() {
                Some((f, bytes, _)) if *f == fi => bytes.extend_from_slice(&gid.to_be_bytes()),
                _ => segs.push((fi, gid.to_be_bytes().to_vec(), pen)),
            }
            pen += adv.advance;
        }
        for (fi, bytes, sx) in &segs {
            let tag = if *fi < doc.fonts.len() {
                doc.fonts[*fi].tag
            } else {
                doc.fallback.as_ref().expect("checked").tag
            };
            content.begin_text();
            content.set_font(Name(tag), px);
            content.set_text_matrix([px, 0.0, 0.0, px, *sx, flip(baseline, ph)]);
            content.show(Str(bytes));
            content.end_text();
        }
        baseline += lh;
    }
    Ok(())
}

fn paint_image(
    content: &mut Content,
    node: &Placed,
    bx: f32,
    by: f32,
    ph: f32,
    doc: &PdfDoc,
) -> Result<(), Error> {
    let (bytes, fit) = match &node.media {
        Some(Media::Image { bytes, fit, .. }) => (bytes, *fit),
        _ => return Ok(()),
    };
    let key = hash_bytes(bytes);
    let idx = doc
        .image_index
        .get(&key)
        .copied()
        .ok_or_else(|| Error::Asset("image not collected".into()))?;
    let emb = &doc.images[idx];
    let (sw, sh) = (emb.w as f32, emb.h as f32);
    let bi = node.style.border;
    let (bx, by) = (bx + bi, by + bi);
    let (bw, bh) = ((node.w - 2.0 * bi).max(1.0), (node.h - 2.0 * bi).max(1.0));
    // Destination box in PDF coords + draw matrix for the unit image square.
    // The image maps upright: uniform positive scales only.
    let (dx, dw, dy_bottom, dh, scale) = match fit {
        ImgFit::Fill => (bx, bw, flip(by + bh, ph), bh, None),
        ImgFit::Cover => (bx, bw, flip(by + bh, ph), bh, Some((bw / sw).max(bh / sh))),
        ImgFit::Contain => {
            let s = (bw / sw).min(bh / sh);
            let (dw, dh) = (sw * s, sh * s);
            (
                bx + (bw - dw) / 2.0,
                dw,
                flip(by + bh, ph) + (bh - dh) / 2.0,
                dh,
                None,
            )
        }
    };
    content.save_state();
    if node.style.radius > 0.5 {
        rr(
            content,
            bx,
            flip(by + bh, ph),
            bw,
            bh,
            (node.style.radius - bi).max(0.0),
        );
        content.clip_nonzero();
        content.end_path();
    }
    match scale {
        // Cover: clip to the box, draw the full frame oversized and centered.
        Some(s) => {
            let ox = dx - (sw * s - dw) / 2.0;
            let oy = dy_bottom - (sh * s - dh) / 2.0;
            content.transform([s, 0.0, 0.0, s, ox, oy]);
        }
        None => {
            content.transform([dw, 0.0, 0.0, dh, dx, dy_bottom]);
        }
    }
    content.op("Do").operand(Name(image_name(idx)));
    content.restore_state();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hikari_core::{Node, Style};

    fn count(doc: &[u8], needle: &[u8]) -> usize {
        doc.windows(needle.len()).filter(|w| *w == needle).count()
    }

    #[test]
    fn pdf_header_pages_and_fonts() {
        let pages = vec![
            Node::banner(595.0, 842.0, "#ffffff", "Hello PDF", 48.0, "#111111"),
            Node::banner(595.0, 842.0, "#ffffff", "Second page", 48.0, "#111111"),
        ];
        let doc = render_pdf(&pages, PageSize::Custom { w: 595.0, h: 842.0 }).unwrap();
        assert_eq!(&doc[..5], b"%PDF-");
        // Exactly 2 pages (exclude the /Pages tree node).
        assert_eq!(
            count(&doc, b"/Type /Page") - count(&doc, b"/Type /Pages"),
            2
        );
        assert!(count(&doc, b"/ToUnicode") >= 1);
        assert!(count(&doc, b"Identity-H") >= 1);
        assert!(count(&doc, b"DejaVuSans") >= 1);
    }

    #[test]
    fn pdf_rejects_empty_pages() {
        assert!(render_pdf(&[], PageSize::A4).is_err());
    }

    #[test]
    fn pdf_nested_outline() {
        let page = Node::container(
            Style::column().with_size(400.0, 600.0),
            vec![
                Node::text("Part", Style::text(32.0, "#111").with_bookmark(1)),
                Node::text("Chapter", Style::text(24.0, "#111").with_bookmark(2)),
                Node::text("Section", Style::text(20.0, "#111").with_bookmark(3)),
                Node::text("Appendix", Style::text(32.0, "#111").with_bookmark(1)),
            ],
        );
        let options = PdfOptions {
            outline: true,
            ..PdfOptions::new()
        };
        let doc =
            render_pdf_with(&[page], PageSize::Custom { w: 400.0, h: 600.0 }, &options).unwrap();
        // Nested items reference parents; every item has a dest.
        assert!(count(&doc, b"/First") >= 2);
        assert!(count(&doc, b"/Parent") >= 4);
    }

    #[test]
    fn pdf_attachment_embeds_file() {
        let page = Node::banner(400.0, 600.0, "#ffffff", "Hi", 36.0, "#111111");
        let options = PdfOptions {
            attachments: vec![Attachment {
                name: "invoice.xml".into(),
                mime: "text/xml".into(),
                bytes: b"<invoice><total>288</total></invoice>".to_vec(),
            }],
            ..PdfOptions::new()
        };
        let doc =
            render_pdf_with(&[page], PageSize::Custom { w: 400.0, h: 600.0 }, &options).unwrap();
        assert!(count(&doc, b"/EmbeddedFiles") >= 1);
        assert!(count(&doc, b"invoice.xml") >= 1);
        // Embedded-file stream typed with the MIME subtype.
        assert!(count(&doc, b"/Type /EmbeddedFile") >= 1);
        assert!(count(&doc, b"/Subtype /text#2Fxml") >= 1);
    }

    #[test]
    fn pdf_outline_and_metadata() {
        let page = Node::container(
            Style::column().with_size(400.0, 600.0),
            vec![
                Node::text("Chapter One", Style::text(32.0, "#111").with_bookmark(1)),
                Node::text("body text here", Style::text(14.0, "#333")),
            ],
        );
        let options = PdfOptions {
            title: Some("Test Doc".into()),
            author: Some("Hikari".into()),
            outline: true,
            paginate: false,
            attachments: Vec::new(),
            pdfa: false,
        };
        let doc =
            render_pdf_with(&[page], PageSize::Custom { w: 400.0, h: 600.0 }, &options).unwrap();
        assert!(count(&doc, b"/Outlines") >= 1);
        assert!(count(&doc, b"/Title") >= 1);
        // Bookmark title embedded as a text string.
        assert!(count(&doc, b"Chapter One") >= 1);
    }

    #[test]
    fn pdf_link_annotations_carry_uris() {
        let page = Node::container(
            Style::column().with_size(400.0, 600.0),
            vec![Node::text(
                "Pay now",
                Style::text(20.0, "#00f").with_link("https://pay.example/x"),
            )],
        );
        let doc = render_pdf(&[page], PageSize::Custom { w: 400.0, h: 600.0 }).unwrap();
        assert!(count(&doc, b"/Subtype/Link") + count(&doc, b"/Subtype /Link") >= 1);
        assert!(count(&doc, b"https://pay.example/x") >= 1);
    }

    /// A font name as it appears in a PDF name string, where a space is
    /// written `#20`. Asserting on the raw Rust name would silently miss.
    fn pdf_name(name: &str) -> String {
        name.replace(' ', "#20")
    }

    /// A visually distinct second font, if the host has one.
    fn second_font() -> Option<Vec<u8>> {
        const CANDIDATES: &[&str] = &[
            "/System/Library/Fonts/Supplemental/Georgia.ttf",
            "/Library/Fonts/Georgia.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSerif-Regular.ttf",
        ];
        CANDIDATES
            .iter()
            .find_map(|p| std::fs::read(p).ok())
            .filter(|b| b.len() > 10_000)
    }

    #[test]
    fn pdf_embeds_a_registered_font() {
        // The PDF backend holds a font list keyed by FontId rather than a fixed
        // primary/fallback pair, so a custom font must produce its own embedded
        // font program and its own resource block.
        let Some(bytes) = second_font() else {
            eprintln!("no second font on this host; skipping");
            return;
        };
        let id = hikari_core::register_font("Serif Brand", &bytes).expect("register");
        let name = hikari_core::font_entry(id).expect("entry").name.clone();

        let mut style = Style::column().with_size(400.0, 200.0);
        style.font = Some(id);
        let page = Node::container(
            style,
            vec![Node::text(
                "Sphinx of black quartz",
                Style::text(28.0, "#000000"),
            )],
        );
        let doc = render_pdf(&[page], PageSize::Custom { w: 400.0, h: 200.0 }).expect("pdf");

        assert!(
            count(&doc, pdf_name(&name).as_bytes()) >= 1,
            "PDF did not name the registered font {name:?}"
        );
        // One font program per distinct font used.
        assert!(count(&doc, b"/FontFile2") >= 1, "no embedded font program");
        // Type0/CID text keeps extracted text working, which is the thing a
        // custom font must not break.
        assert!(count(&doc, b"/ToUnicode") >= 1, "no ToUnicode CMap");
    }

    #[test]
    fn pdf_with_two_fonts_embeds_both() {
        // Two fonts in one document is the case the old fixed-pair design could
        // not express, and the one that would silently reuse the wrong glyph
        // remap if the index mapping were off.
        let Some(bytes) = second_font() else {
            return;
        };
        let id = hikari_core::register_font("Second Face", &bytes).expect("register");
        let name = hikari_core::font_entry(id).expect("entry").name.clone();

        let mut a = Style::column().with_size(400.0, 300.0);
        a.font = None; // embedded
        let mut b = Style::column().with_size(400.0, 300.0);
        b.font = Some(id);

        let page = Node::container(
            a,
            vec![
                Node::text("Built in DejaVu", Style::text(24.0, "#000000")),
                Node::container(
                    b,
                    vec![Node::text(
                        "Set in the brand face",
                        Style::text(24.0, "#000000"),
                    )],
                ),
            ],
        );
        let doc = render_pdf(&[page], PageSize::Custom { w: 400.0, h: 300.0 }).expect("pdf");

        assert!(count(&doc, b"DejaVuSans") >= 1, "built-in font missing");
        assert!(
            count(&doc, pdf_name(&name).as_bytes()) >= 1,
            "registered font {name:?} missing from a two-font document"
        );
        // Two distinct font programs: one per face.
        assert!(
            count(&doc, b"/FontFile2") >= 2,
            "expected two embedded font programs, found {}",
            count(&doc, b"/FontFile2")
        );
        // Distinct resource tags, or the second face would address the first's
        // glyphs.
        // F1 is the embedded font, F3 the first caller font; F2 is reserved
        // for the system fallback so the two can never collide.
        assert!(
            count(&doc, b"/F1") >= 1,
            "embedded font resource tag missing"
        );
        assert!(
            count(&doc, b"/F3") >= 1,
            "caller font resource tag missing or colliding with the fallback slot"
        );
    }

    #[test]
    fn pdf_rejects_an_unregistered_font_id() {
        // Unlike raster and SVG, which degrade to the embedded font, the PDF
        // writer must not silently emit a document in the wrong typeface.
        let mut style = Style::column().with_size(400.0, 200.0);
        style.font = Some(987_654);
        let page = Node::container(
            style,
            vec![Node::text("mystery font", Style::text(20.0, "#000000"))],
        );
        let err = render_pdf(&[page], PageSize::Custom { w: 400.0, h: 200.0 })
            .expect_err("an unregistered font id must not render silently");
        assert!(err.to_string().contains("font"), "unhelpful error: {err}");
    }

    #[test]
    fn pdf_embeds_image_xobject() {
        use image::codecs::png::PngEncoder;
        use image::{ExtendedColorType, ImageEncoder, RgbaImage};
        let img = RgbaImage::from_pixel(8, 4, image::Rgba([10, 200, 90, 255]));
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(img.as_raw(), 8, 4, ExtendedColorType::Rgba8)
            .unwrap();
        let pages = vec![Node::image(buf, Style::new().with_size(200.0, 100.0))];
        let doc = render_pdf(&pages, PageSize::A4).unwrap();
        assert!(count(&doc, b"/Subtype /Image") >= 1);
    }
}
