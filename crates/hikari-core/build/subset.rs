//! Layout-preserving TrueType subsetter used at build time.
//!
//! # Why this exists
//!
//! The obvious way to shrink an embedded font is to hand it to a generic
//! subsetter and ask for a pile of tables. That silently produces a font with
//! **no shaping**: `GSUB`, `GPOS` and `GDEF` either get dropped or come out
//! with glyph ids that no longer point at the right outlines. The result
//! renders Latin fine and quietly breaks everything else -- Arabic joining,
//! Hebrew joining, kerning and ligatures all vanish, and nothing errors.
//!
//! # The trick that makes this small
//!
//! Layout tables address glyphs by **glyph id**. So instead of renumbering
//! glyphs into a dense range (which invalidates every one of those references
//! and would force us to rewrite `GSUB`/`GPOS`/`GDEF` field by field), we keep
//! the original ids and simply leave the gaps empty:
//!
//! * `glyf` / `loca` keep a zero-length entry for every dropped id,
//! * `hmtx` keeps a zeroed metric for every dropped id,
//! * `GSUB` / `GPOS` / `GDEF` are copied **byte for byte**.
//!
//! The cost is a sparse id space, which for DejaVu Sans is about 49 KB of
//! `loca` + `hmtx` padding. In exchange the layout tables need no rewriting at
//! all, so there is no field we can silently forget to remap -- the class of
//! bug that makes hand-rolled layout subsetters so fragile.
//!
//! Two other details matter for correctness:
//!
//! * **Composite glyph closure.** 866 of the glyphs we keep are composites
//!   that reference 360 further glyphs by id. Those components must be pulled
//!   in or the outline renders truncated.
//! * **`post` is rewritten to format 3.0.** The original carries 62 KB of
//!   glyph *names*, which nothing in the render path reads.

use std::collections::BTreeSet;

fn rd_u16(b: &[u8], o: usize) -> u16 {
    u16::from_be_bytes([b[o], b[o + 1]])
}

fn rd_i16(b: &[u8], o: usize) -> i16 {
    i16::from_be_bytes([b[o], b[o + 1]])
}

fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn w_u16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

fn w_i16(v: i16) -> [u8; 2] {
    v.to_be_bytes()
}

fn w_u32(v: u32) -> [u8; 4] {
    v.to_be_bytes()
}

/// A parsed sfnt table directory.
pub struct Font {
    data: Vec<u8>,
    tables: Vec<(String, usize, usize)>,
}

impl Font {
    /// Parse the table directory of a single-font sfnt.
    pub fn parse(data: Vec<u8>) -> Font {
        let num = rd_u16(&data, 4) as usize;
        let mut tables = Vec::with_capacity(num);
        for i in 0..num {
            let o = 12 + i * 16;
            tables.push((
                String::from_utf8_lossy(&data[o..o + 4]).to_string(),
                rd_u32(&data, o + 8) as usize,
                rd_u32(&data, o + 12) as usize,
            ));
        }
        tables.sort_by(|a, b| a.0.cmp(&b.0));
        Font { data, tables }
    }

    /// Borrow a table by four-character tag.
    pub fn get(&self, tag: &str) -> Option<&[u8]> {
        self.tables
            .iter()
            .find(|t| t.0 == tag)
            .map(|t| &self.data[t.1..t.1 + t.2])
    }
}

/// Build a `cmap` format 4 subtable from codepoint -> glyph id pairs.
///
/// Uses `idDelta` exclusively (`idRangeOffset` stays zero) and emits one
/// segment per maximal run where the codepoint *and* the glyph id both advance
/// by one, so the table stays small without needing a glyph index array.
fn build_cmap4(pairs: &[(u32, u16)]) -> Vec<u8> {
    let mut sorted: Vec<(u32, u16)> = pairs.to_vec();
    sorted.sort_by_key(|p| p.0);
    sorted.dedup_by_key(|p| p.0);

    // (first codepoint, last codepoint, glyph id of the first codepoint)
    let mut segs: Vec<(u32, u32, u16)> = Vec::new();
    for (cp, gid) in &sorted {
        match segs.last_mut() {
            Some(last)
                if last.1 + 1 == *cp
                    && u32::from(last.2) + (last.1 - last.0) + 1 == u32::from(*gid) =>
            {
                last.1 = *cp;
            }
            _ => segs.push((*cp, *cp, *gid)),
        }
    }
    // The format requires a final 0xFFFF segment.
    segs.push((0xFFFF, 0xFFFF, 0));

    let seg_count = segs.len();
    let seg_count_x2 = (seg_count * 2) as u16;
    let mut entry_selector = 0u16;
    while (1u32 << (entry_selector + 1)) <= seg_count as u32 {
        entry_selector += 1;
    }
    let search_range = 2u16 << entry_selector;
    let range_shift = seg_count_x2.wrapping_sub(search_range);
    let length = 16 + seg_count * 8;

    let mut out = Vec::with_capacity(length);
    out.extend(w_u16(4)); // format
    out.extend(w_u16(length as u16));
    out.extend(w_u16(0)); // language
    out.extend(w_u16(seg_count_x2));
    out.extend(w_u16(search_range));
    out.extend(w_u16(entry_selector));
    out.extend(w_u16(range_shift));
    for (_, end, _) in &segs {
        out.extend(w_u16(*end as u16));
    }
    out.extend(w_u16(0)); // reservedPad
    for (start, _, _) in &segs {
        out.extend(w_u16(*start as u16));
    }
    for (start, _, gid) in &segs {
        out.extend(w_i16((*gid as i32 - *start as i32) as i16));
    }
    for _ in &segs {
        out.extend(w_u16(0)); // idRangeOffset
    }
    debug_assert_eq!(out.len(), length);
    out
}

/// Wrap a format 4 subtable in a full `cmap` table.
fn wrap_cmap(sub: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend(w_u16(0)); // version
    out.extend(w_u16(2)); // Windows BMP + Unicode BMP
    let offset = (4 + 2 * 8) as u32;
    for (platform, encoding) in [(3u16, 1u16), (0u16, 3u16)] {
        out.extend(w_u16(platform));
        out.extend(w_u16(encoding));
        out.extend(w_u32(offset));
    }
    out.extend_from_slice(sub);
    out
}

/// Codepoint ranges the embedded font must cover.
///
/// Latin, Greek, Cyrillic, Hebrew and Arabic plus the Arabic presentation
/// forms (needed because the shaping tables map base letters onto them), then
/// punctuation, currency, arrows, math and symbols. CJK is deliberately absent
/// and falls back to a system font at runtime.
pub const RANGES: &[(u32, u32)] = &[
    (0x0020, 0x007E), // ASCII
    (0x00A0, 0x00FF), // Latin-1 supplement
    (0x0100, 0x024F), // Latin extended A/B
    (0x0370, 0x03FF), // Greek
    (0x0400, 0x04FF), // Cyrillic
    (0x0590, 0x05FF), // Hebrew
    (0x0600, 0x06FF), // Arabic
    (0x2000, 0x206F), // General punctuation
    (0x2070, 0x209F), // Super/subscripts
    (0x20A0, 0x20CF), // Currency
    (0x2100, 0x214F), // Letterlike symbols
    (0x2190, 0x21FF), // Arrows
    (0x2200, 0x22FF), // Mathematical operators
    (0x25A0, 0x25FF), // Geometric shapes
    (0x2600, 0x26FF), // Miscellaneous symbols
    (0x2700, 0x27BF), // Dingbats
    (0xFB00, 0xFB4F), // Alphabetic presentation forms: fi/fl/ffi ligatures
    (0xFB50, 0xFDFF), // Arabic presentation forms-A
    (0xFE70, 0xFEFF), // Arabic presentation forms-B
];

/// Codepoint ranges for the optional CJK face.
///
/// Kept separate from [`RANGES`] because CJK is a cargo feature: a full CJK
/// coverage set is measured in megabytes, which is unacceptable inside a
/// 2.8 MB WebAssembly build but reasonable for a server that ships native.
///
/// The three tiers are the pragmatic choice, not a principled one — they follow
/// how much of the repertoire each successive set of scripts actually needs.
/// Measured on `NotoSansSC[wght].ttf`, see `BENCHMARKS.md` for the byte counts
/// that justify the cut points.
pub const CJK_RANGES: &[(u32, u32)] = &[
    (0x0020, 0x007E), // ASCII: a CJK document still has Latin in it
    (0x00A0, 0x00FF), // Latin-1 supplement, for the same reason
    (0x2000, 0x206F), // General punctuation: the CJK fullwidth forms live here
    (0x3000, 0x303F), // CJK symbols and punctuation
    (0x3040, 0x309F), // Hiragana
    (0x30A0, 0x30FF), // Katakana
    (0x31F0, 0x31FF), // Katakana phonetic extensions
    (0x3400, 0x4DBF), // CJK unified ideographs extension A
    (0x4E00, 0x9FFF), // CJK unified ideographs
    (0xF900, 0xFAFF), // CJK compatibility ideographs
    (0xFF00, 0xFFEF), // Halfwidth and fullwidth forms
];

/// What the subsetter produced, for build-time logging and tests.
#[derive(Debug, Clone, Copy)]
pub struct SubsetStats {
    /// Glyph ids carried through (composites included).
    pub glyphs: usize,
    /// Codepoints reachable through the new `cmap`.
    pub codepoints: usize,
    /// Bytes in the source font.
    pub source_bytes: usize,
    /// Bytes in the subset font.
    pub subset_bytes: usize,
    /// Whether `GSUB`/`GPOS`/`GDEF` were carried over verbatim.
    pub layout_preserved: bool,
}

fn err(msg: &str) -> String {
    msg.to_string()
}

/// Subset `src` to [`RANGES`], preserving glyph ids and layout tables.
///
/// A convenience wrapper over [`subset_ranges`] for callers that want the
/// Latin-covering default. The build script calls [`subset_ranges`] directly,
/// once per bundled face.
#[allow(dead_code)]
pub fn subset(src: &[u8]) -> Result<(Vec<u8>, SubsetStats), String> {
    subset_ranges(src, RANGES)
}

/// Subset `src` to `ranges`, preserving glyph ids and layout tables.
///
/// Every embedded face goes through this one function. The Latin-covering
/// default uses [`RANGES`]; the optional CJK face uses [`CJK_RANGES`]. Keeping
/// a single implementation is deliberate — a second subsetter would be a second
/// place for the layout-table bug to reappear, and that bug is silent.
pub fn subset_ranges(src: &[u8], ranges: &[(u32, u32)]) -> Result<(Vec<u8>, SubsetStats), String> {
    let font = Font::parse(src.to_vec());

    let maxp = font.get("maxp").ok_or_else(|| err("missing maxp"))?;
    let num_glyphs = usize::from(rd_u16(maxp, 4));
    let head = font.get("head").ok_or_else(|| err("missing head"))?;
    let index_to_loc = rd_i16(head, 50);
    let loca = font.get("loca").ok_or_else(|| err("missing loca"))?;
    let glyf = font.get("glyf").ok_or_else(|| err("missing glyf"))?;
    let hmtx = font.get("hmtx").ok_or_else(|| err("missing hmtx"))?;
    let hhea = font.get("hhea").ok_or_else(|| err("missing hhea"))?;
    let num_h_metrics = usize::from(rd_u16(hhea, 34));

    // `loca` holds `num_glyphs + 1` entries; read defensively so a malformed
    // source degrades to an empty glyph instead of a build-time panic.
    let loca_entries = loca.len() / if index_to_loc == 0 { 2 } else { 4 };
    let loca_at = |g: usize| -> usize {
        if g >= loca_entries {
            return glyf.len();
        }
        if index_to_loc == 0 {
            usize::from(rd_u16(loca, g * 2)) * 2
        } else {
            rd_u32(loca, g * 4) as usize
        }
    };

    // --- resolve the source cmap (format 4) --------------------------------
    let cmap = font.get("cmap").ok_or_else(|| err("missing cmap"))?;
    let num_subtables = usize::from(rd_u16(cmap, 2));
    let mut best: Option<(usize, usize)> = None;
    for i in 0..num_subtables {
        let record = 4 + i * 8;
        let off = rd_u32(cmap, record + 4) as usize;
        if off + 4 <= cmap.len() && rd_u16(cmap, off) == 4 {
            let len = usize::from(rd_u16(cmap, off + 2));
            if best.is_none_or(|(_, bl)| len > bl) {
                best = Some((off, len));
            }
        }
    }
    let (cmap_off, _) = best.ok_or_else(|| err("no cmap format 4 subtable"))?;
    let seg_x2 = usize::from(rd_u16(cmap, cmap_off + 6));
    let seg_count = seg_x2 / 2;
    let end_at = cmap_off + 14;
    let start_at = end_at + seg_x2 + 2;
    let delta_at = start_at + seg_x2;
    let range_at = delta_at + seg_x2;

    let lookup = |cp: u32| -> Option<u16> {
        let c = cp as u16;
        for s in 0..seg_count {
            if c > rd_u16(cmap, end_at + s * 2) {
                continue;
            }
            let start = rd_u16(cmap, start_at + s * 2);
            if c < start {
                return None;
            }
            let delta = rd_u16(cmap, delta_at + s * 2);
            let range_offset = rd_u16(cmap, range_at + s * 2);
            if range_offset == 0 {
                return Some(c.wrapping_add(delta));
            }
            let at = range_at + s * 2 + usize::from(range_offset) + usize::from(c - start) * 2;
            if at + 1 >= cmap.len() {
                return None;
            }
            let gid = rd_u16(cmap, at);
            if gid == 0 {
                return None;
            }
            return Some(gid.wrapping_add(delta));
        }
        None
    };

    // --- seed the glyph set from the cmap over our ranges -------------------
    let mut keep: BTreeSet<u16> = BTreeSet::from([0]);
    let mut pairs: Vec<(u32, u16)> = Vec::new();
    for (lo, hi) in ranges {
        for cp in *lo..=*hi {
            if let Some(gid) = lookup(cp) {
                if usize::from(gid) < num_glyphs {
                    keep.insert(gid);
                    pairs.push((cp, gid));
                }
            }
        }
    }
    let codepoints = pairs.len();

    // --- composite glyph closure -------------------------------------------
    // Composite outlines reference component glyphs by id. Those references
    // are copied verbatim along with the rest of the outline, so every
    // component has to survive or the glyph renders truncated.
    loop {
        let mut newly: BTreeSet<u16> = BTreeSet::new();
        for &gid in &keep {
            let start = loca_at(usize::from(gid));
            let end = loca_at(usize::from(gid) + 1);
            if end <= start + 10 || rd_i16(glyf, start) >= 0 {
                continue; // simple glyph, or empty
            }
            let mut p = start + 10;
            while p + 4 <= end {
                let flags = rd_u16(glyf, p);
                let index = rd_u16(glyf, p + 2);
                if usize::from(index) < num_glyphs && !keep.contains(&index) {
                    newly.insert(index);
                }
                p += 4;
                p += if flags & 0x0001 != 0 { 4 } else { 2 };
                if flags & 0x0008 != 0 {
                    p += 2;
                } else if flags & 0x0040 != 0 {
                    p += 4;
                } else if flags & 0x0080 != 0 {
                    p += 8;
                }
                if flags & 0x0020 == 0 {
                    break; // MORE_COMPONENTS clear
                }
            }
        }
        if newly.is_empty() {
            break;
        }
        keep.extend(newly);
    }

    // --- widen the id space to cover layout references ---------------------
    // `fontdue` eagerly instantiates every glyph named anywhere in GSUB, and
    // errors out if any of those ids is past `numGlyphs`. GSUB in DejaVu
    // covers scripts we do not ship (Cyrillic and math lookups, say), so the
    // id space has to reach those ids even though we keep no outline for them
    // -- they land on zero-length entries and rasterize blank, which is correct
    // for codepoints outside our coverage anyway.
    //
    // Clamping at the source `numGlyphs` is what keeps this honest: inside a
    // well-formed font any u16 below that bound may legitimately be a glyph
    // id, and anything above it is an offset or a count, not a glyph.
    let mut max_layout_id = 0u16;
    for tag in ["GSUB", "GPOS", "GDEF"] {
        let Some(data) = font.get(tag) else { continue };
        let mut i = 0;
        while i + 1 < data.len() {
            let value = rd_u16(data, i);
            if value > 0 && usize::from(value) < num_glyphs {
                max_layout_id = max_layout_id.max(value);
            }
            i += 1;
        }
    }

    // --- rebuild glyf / loca / hmtx on the original id space ---------------
    let new_num_glyphs =
        usize::from(*keep.iter().max().unwrap()).max(usize::from(max_layout_id)) + 1;
    if new_num_glyphs > u16::MAX as usize {
        return Err("glyph id space exceeds u16".into());
    }

    let mut new_glyf: Vec<u8> = Vec::new();
    let mut new_loca: Vec<u8> = Vec::with_capacity((new_num_glyphs + 1) * 4);
    for gid in 0..new_num_glyphs {
        new_loca.extend(w_u32(new_glyf.len() as u32));
        if !keep.contains(&(gid as u16)) {
            continue; // leave a zero-length gap so ids stay stable
        }
        // Guard the source bounds: `loca` is only valid up to the source
        // glyph count, and the id space above that exists purely to satisfy
        // layout references.
        if gid + 1 >= num_glyphs {
            continue;
        }
        let start = loca_at(gid);
        let end = loca_at(gid + 1);
        if end > start && end <= glyf.len() {
            new_glyf.extend_from_slice(&glyf[start..end]);
            if new_glyf.len() % 2 == 1 {
                new_glyf.push(0); // outlines are 2-byte aligned
            }
        }
    }
    new_loca.extend(w_u32(new_glyf.len() as u32));

    // `hmtx` layout: the first `numberOfHMetrics` entries are 4 bytes each
    // (advance + lsb); every glyph past that is a 2-byte lsb only, and the
    // advance repeats the last long entry. Ids past the source glyph count have
    // nothing to copy -- they exist only to keep layout references in bounds.
    let last_advance = {
        let at = num_h_metrics.saturating_sub(1) * 4;
        if num_h_metrics > 0 && at + 1 < hmtx.len() {
            rd_u16(hmtx, at)
        } else {
            0
        }
    };
    let mut new_hmtx: Vec<u8> = Vec::with_capacity(new_num_glyphs * 4);
    for gid in 0..new_num_glyphs {
        let (advance, lsb) = if gid < num_h_metrics {
            let at = gid * 4;
            let advance = if at + 1 < hmtx.len() {
                rd_u16(hmtx, at)
            } else {
                0
            };
            let lsb = if at + 3 < hmtx.len() {
                rd_i16(hmtx, at + 2)
            } else {
                0
            };
            (advance, lsb)
        } else if gid < num_glyphs {
            let at = num_h_metrics * 4 + (gid - num_h_metrics) * 2;
            let lsb = if at + 1 < hmtx.len() {
                rd_i16(hmtx, at)
            } else {
                0
            };
            (last_advance, lsb)
        } else {
            (0, 0)
        };
        new_hmtx.extend(w_u16(advance));
        new_hmtx.extend(w_i16(lsb));
    }

    let mut new_head = head.to_vec();
    new_head[50..52].copy_from_slice(&w_u16(1)); // long loca
    let mut new_maxp = maxp.to_vec();
    new_maxp[4..6].copy_from_slice(&w_u16(new_num_glyphs as u16));
    let mut new_hhea = hhea.to_vec();
    new_hhea[34..36].copy_from_slice(&w_u16(new_num_glyphs as u16));

    // post format 3.0: no glyph names (the source carries 62 KB of them).
    let mut post = vec![0u8; 32];
    post[0..4].copy_from_slice(&w_u32(0x0003_0000));

    let mut out: Vec<(String, Vec<u8>)> = vec![
        (
            "OS/2".into(),
            font.get("OS/2")
                .ok_or_else(|| err("missing OS/2"))?
                .to_vec(),
        ),
        ("cmap".into(), wrap_cmap(&build_cmap4(&pairs))),
        ("glyf".into(), new_glyf),
        ("head".into(), new_head),
        ("hhea".into(), new_hhea),
        ("hmtx".into(), new_hmtx),
        ("loca".into(), new_loca),
        ("maxp".into(), new_maxp),
        (
            "name".into(),
            font.get("name")
                .ok_or_else(|| err("missing name"))?
                .to_vec(),
        ),
        ("post".into(), post),
    ];

    // Layout tables, verbatim. Valid precisely because ids did not move.
    let mut layout_preserved = true;
    for tag in ["GSUB", "GPOS", "GDEF"] {
        match font.get(tag) {
            Some(data) => out.push((tag.to_string(), data.to_vec())),
            None => layout_preserved = false,
        }
    }
    // The legacy `kern` table is deliberately not carried: DejaVu ships both
    // it and GPOS, every modern shaper prefers GPOS, and dropping it saves
    // 16 KB with no measurable change in shaped output.
    out.sort_by(|a, b| a.0.cmp(&b.0));

    // --- assemble the sfnt --------------------------------------------------
    let count = out.len();
    let mut search_range = 16u32;
    let mut entry_selector = 0u32;
    while search_range * 2 <= count as u32 * 16 {
        search_range *= 2;
        entry_selector += 1;
    }

    let mut data: Vec<u8> = Vec::new();
    data.extend(w_u32(0x0001_0000));
    data.extend(w_u16(count as u16));
    data.extend(w_u16(search_range as u16));
    data.extend(w_u16(entry_selector as u16));
    data.extend(w_u16(
        ((count as u32 * 16).wrapping_sub(search_range)) as u16,
    ));

    let mut offset = 12 + count * 16;
    for (tag, body) in &out {
        data.extend(tag.as_bytes());
        data.extend(w_u32(0)); // checksum, not validated by consumers
        data.extend(w_u32(offset as u32));
        data.extend(w_u32(body.len() as u32));
        offset += (body.len() + 3) & !3;
    }
    for (_, body) in &out {
        data.extend_from_slice(body);
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
    }

    let stats = SubsetStats {
        glyphs: keep.len(),
        codepoints,
        source_bytes: src.len(),
        subset_bytes: data.len(),
        layout_preserved,
    };
    Ok((data, stats))
}
