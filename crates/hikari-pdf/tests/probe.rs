#[test]
fn probe_subset_profiles() {
    use allsorts::binary::read::ReadScope;
    use allsorts::font_data::FontData;
    use allsorts::subset::{subset, CmapTarget, SubsetProfile};
    use allsorts::tag;
    let bytes = hikari_core::font_bytes();
    // All currently-used glyphs: collect from representative strings.
    let mut gids = std::collections::BTreeSet::new();
    gids.insert(0u16);
    let face = ttf_parser::Face::parse(bytes, 0).unwrap();
    let mut chars = std::collections::BTreeSet::new();
    for s in [
        "Hello from Hikari",
        "ñשم—€ AVfi",
        "0123456789.,:$%!?\u{2713}\u{2192}",
    ] {
        for ch in s.chars() {
            if let Some(g) = face.glyph_index(ch) {
                gids.insert(g.0);
                chars.insert(ch);
            }
        }
    }
    println!("seed glyphs: {}", gids.len());
    for (name, profile) in [
        ("Pdf", SubsetProfile::Pdf),
        (
            "Custom",
            SubsetProfile::Custom(vec![
                tag::HEAD,
                tag::HHEA,
                tag::MAXP,
                tag::CMAP,
                tag::HMTX,
                tag::LOCA,
                tag::GLYF,
                tag::NAME,
                tag::POST,
                tag::OS_2,
                tag::GSUB,
                tag::GPOS,
                tag::GDEF,
            ]),
        ),
    ] {
        let scope = ReadScope::new(bytes);
        let ff = scope.read::<FontData<'_>>().unwrap();
        let prov = ff.table_provider(0).unwrap();
        let ids: Vec<u16> = gids.iter().copied().collect();
        match subset(&prov, &ids, &profile, CmapTarget::Unicode) {
            Ok(b) => println!(
                "{name}: {} bytes tables={}",
                b.len(),
                table_tags(&b).join(",")
            ),
            Err(e) => println!("{name}: ERR {e:?}"),
        }
    }
}

fn table_tags(bytes: &[u8]) -> Vec<String> {
    if bytes.len() < 12 {
        return vec![];
    }
    let n = u16::from_be_bytes([bytes[4], bytes[5]]) as usize;
    (0..n)
        .filter_map(|i| {
            let o = 12 + i * 16;
            if o + 4 > bytes.len() {
                return None;
            }
            Some(String::from_utf8_lossy(&bytes[o..o + 4]).to_string())
        })
        .collect()
}
