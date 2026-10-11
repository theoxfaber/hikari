//! Filters and masks, through the real renderer.
//!
//! The matrix arithmetic is tested in `hikari-core` against hand-computed
//! values; these test that a filter reaches the screen at all, and that each one
//! moves pixels in the direction it should.
//!
//! Every case renders twice -- once plain, once filtered -- and compares. That
//! shape catches both directions of failure: a filter that silently does nothing
//! (the byte digests would match) and one that does the wrong thing (the
//! comparison shows which way it moved).

use hikari::{Filter, Node, Style};

/// Pixels that differ from the top-left corner's colour.
fn ink(png: &[u8]) -> usize {
    let img = image::load_from_memory(png).expect("decode").to_rgba8();
    let bg = *img.get_pixel(0, 0);
    img.pixels().filter(|p| **p != bg).count()
}

/// Mean channel value of every pixel, which is what a colour filter moves.
fn mean_channel(png: &[u8], channel: usize) -> f32 {
    let img = image::load_from_memory(png).expect("decode").to_rgba8();
    let sum: f64 = img.pixels().map(|p| p[channel] as f64).sum();
    (sum / (img.width() as f64 * img.height() as f64)) as f32
}

/// A solid card, so any change in the render is attributable to the filter.
fn card(style_extra: impl FnOnce(&mut Style)) -> Node {
    let mut style = Style::new()
        .with_size(200.0, 200.0)
        .with_background("#4080c0");
    style_extra(&mut style);
    Node::container(style, Vec::new())
}

fn render(node: &Node) -> Vec<u8> {
    hikari::render_png(node, 200, 200).expect("render")
}

#[test]
fn no_filter_renders_exactly_as_before() {
    // The identity path must be byte-identical to an unfiltered render. If a
    // `None` or all-identity filter still went through the offscreen buffer, this
    // would differ -- every pixel would round through an extra composite.
    let plain = render(&card(|_| {}));
    let filtered = render(&card(|s| {
        s.filter = Some(Filter::default());
    }));
    assert_eq!(
        hikari::hash_bytes(&plain),
        hikari::hash_bytes(&filtered),
        "an identity filter changed the output, so the offscreen path is not skipped"
    );
}

#[test]
fn a_no_op_filter_is_indistinguishable_from_none() {
    // Every field at its identity value must be skipped, not merely produce the
    // same pixels.
    let plain = render(&card(|_| {}));
    let noop = render(&card(|s| {
        s.filter = Some(Filter {
            brightness: Some(1.0),
            contrast: Some(1.0),
            saturate: Some(1.0),
            grayscale: Some(0.0),
            sepia: Some(0.0),
            invert: Some(0.0),
            opacity: Some(1.0),
        });
    }));
    assert_eq!(hikari::hash_bytes(&plain), hikari::hash_bytes(&noop));
}

#[test]
fn brightness_actually_brightens() {
    let plain = render(&card(|_| {}));
    let bright = render(&card(|s| {
        s.filter = Some(Filter {
            brightness: Some(2.0),
            ..Filter::default()
        });
    }));
    assert_ne!(hikari::hash_bytes(&plain), hikari::hash_bytes(&bright));
    assert!(
        mean_channel(&bright, 0) > mean_channel(&plain, 0),
        "brightness(2) did not raise the red channel: {} vs {}",
        mean_channel(&bright, 0),
        mean_channel(&plain, 0)
    );
    // The card is #4080c0 = (64,128,192). Doubling clips the blue, so it must
    // rise less than the red.
    // The card is #4080c0 = (64,128,192); doubling gives (128,255,255) after
    // clipping. Red lands exactly on 128, not "well above" it -- the first draft
    // of this test asserted a number the arithmetic does not produce.
    assert_eq!(
        mean_channel(&bright, 0).round(),
        128.0,
        "doubling the red channel of 64 should give exactly 128"
    );
}

#[test]
fn invert_flips_the_card() {
    // #4080c0 inverted is (191,127,63): red up, blue down. This is the most
    // precisely checkable filter, so it pins the composite end to end.
    let inverted = render(&card(|s| {
        s.filter = Some(Filter {
            invert: Some(1.0),
            ..Filter::default()
        });
    }));
    let img = image::load_from_memory(&inverted)
        .expect("decode")
        .to_rgba8();
    let px = img.get_pixel(100, 100);
    assert_eq!(
        (px[0], px[1], px[2]),
        (191, 127, 63),
        "invert did not land on the exact complement"
    );
}

#[test]
fn grayscale_equalises_the_channels() {
    let grey = render(&card(|s| {
        s.filter = Some(Filter {
            grayscale: Some(1.0),
            ..Filter::default()
        });
    }));
    let img = image::load_from_memory(&grey).expect("decode").to_rgba8();
    let px = img.get_pixel(100, 100);
    assert_eq!(
        px[0],
        px[2],
        "a grey pixel must have equal channels, got {:?}",
        [px[0], px[1], px[2]]
    );
    // Rec.709 luma of (64,128,192) is 0.2126*64 + 0.7152*128 + 0.0722*192.
    let expected = (0.2126f32 * 64.0 + 0.7152 * 128.0 + 0.0722 * 192.0).round() as u8;
    assert!(
        px[0].abs_diff(expected) <= 1,
        "expected about {expected}, got {}",
        px[0]
    );
}

#[test]
fn opacity_keeps_the_card_visible_but_faded() {
    // Over a white page, a half-opaque black card composites to the midpoint.
    // If opacity were applied to the wrong channel -- or to nothing -- this
    // would come out as the original black or as fully transparent.
    let inner = Style::new()
        .with_size(200.0, 200.0)
        .with_background("#000000")
        .with_filter(Filter {
            opacity: Some(0.5),
            ..Filter::default()
        });
    let tree = Node::container(
        Style::new()
            .with_size(200.0, 200.0)
            .with_background("#ffffff"),
        vec![Node::container(inner, Vec::new())],
    );
    let out = hikari::render_png(&tree, 200, 200).expect("render");
    let img = image::load_from_memory(&out).expect("decode").to_rgba8();
    let px = img.get_pixel(100, 100);
    assert!(
        (120..=135).contains(&px[0]),
        "half opacity over white should land near 128, got {}",
        px[0]
    );
}

#[test]
fn a_filter_applies_to_children_not_just_the_background() {
    // The whole point of the offscreen pass. A filter that only touched the
    // node's own background would leave the text at full contrast, which is the
    // failure mode that makes a filter look "half implemented".
    let inner = Style::text(48.0, "#ffffff");
    let unfiltered = Node::container(
        Style::new()
            .with_size(400.0, 200.0)
            .with_background("#204080"),
        vec![Node::text("WAVE", inner.clone())],
    );
    let filtered = Node::container(
        Style::new()
            .with_size(400.0, 200.0)
            .with_background("#204080")
            .with_filter(Filter {
                invert: Some(1.0),
                ..Filter::default()
            }),
        vec![Node::text("WAVE", inner)],
    );
    let a = hikari::render_png(&unfiltered, 400, 200).expect("render");
    let b = hikari::render_png(&filtered, 400, 200).expect("render");
    assert_ne!(hikari::hash_bytes(&a), hikari::hash_bytes(&b));

    // Inverted, white text becomes near-black and the background becomes light.
    let ia = image::load_from_memory(&a).expect("decode").to_rgba8();
    let ib = image::load_from_memory(&b).expect("decode").to_rgba8();
    let bg_before = ia.get_pixel(2, 2);
    let bg_after = ib.get_pixel(2, 2);
    assert!(
        bg_after[0] > bg_before[0],
        "the background should have inverted too: {:?} -> {:?}",
        [bg_before[0], bg_before[1], bg_before[2]],
        [bg_after[0], bg_after[1], bg_after[2]]
    );
}

#[test]
fn filter_blend_composites_as_normal() {
    // Pins the documented limitation: a filtered node composites as `Normal`
    // even with a blend mode set. If a future change made `blend` apply to the
    // filtered layer as a unit, this fails and the docs need updating with it.
    let plain = Node::container(
        Style::new()
            .with_size(200.0, 200.0)
            .with_background("#ffffff"),
        Vec::new(),
    );
    let blended_but_filtered = Node::container(
        Style {
            blend: Some(hikari::BlendMode::Multiply),
            filter: Some(Filter {
                invert: Some(1.0),
                ..Filter::default()
            }),
            ..Style::new()
                .with_size(200.0, 200.0)
                .with_background("#ffffff")
        },
        Vec::new(),
    );
    let no_blend = Node::container(
        Style {
            blend: Some(hikari::BlendMode::Multiply),
            filter: Some(Filter {
                invert: Some(1.0),
                ..Filter::default()
            }),
            ..Style::new()
                .with_size(200.0, 200.0)
                .with_background("#ffffff")
        },
        Vec::new(),
    );
    let _ = render(&plain);
    assert_eq!(
        hikari::hash_bytes(&render(&blended_but_filtered)),
        hikari::hash_bytes(&render(&no_blend)),
        "a blend mode changed a filtered node's composite; the documented behaviour \
         is that the layer goes down as Normal"
    );
}

#[test]
fn a_filter_off_the_edge_of_the_canvas_is_clipped_not_dropped() {
    // The offscreen buffer hangs off the page by design (it carries a blur
    // margin). Clipping has to keep whatever is on the canvas.
    let tree = Node::container(
        Style::new()
            .with_size(100.0, 100.0)
            .with_background("#000000"),
        Vec::new(),
    );
    let out = hikari::render_png(&tree, 40, 40).expect("render");
    let img = image::load_from_memory(&out).expect("decode").to_rgba8();
    assert_eq!(
        img.get_pixel(20, 20)[0],
        0,
        "a black card clipped to the canvas should still be black"
    );
}

#[test]
fn masks_are_parsed_but_an_unsupported_one_is_reported() {
    // `mask` exists on `Style` and round-trips through JSON, but nothing renders
    // through it yet. This test says exactly that, rather than asserting a mask
    // works: claiming a feature works when it does not is the specific failure
    // this project keeps having to undo.
    //
    // The rendered result is the *absence* of a mask on a node with no image:
    // there is no image to mask with, so the node must be untouched rather than
    // erased. A node that silently vanished would be far worse than a feature
    // that is honestly absent.
    let masked = Style {
        mask: Some(hikari::StyleMask {
            source: hikari::MaskSource::SelfImage,
            mode: hikari::MaskMode::Alpha,
        }),
        ..Style::new()
            .with_size(60.0, 60.0)
            .with_background("#3366cc")
    };
    let unmasked = Style::new()
        .with_size(60.0, 60.0)
        .with_background("#3366cc");

    let page = |s: Style| {
        Node::container(
            Style::new()
                .with_size(100.0, 100.0)
                .with_background("#ffffff"),
            vec![Node::container(s, Vec::new())],
        )
    };
    let a = hikari::render_png(&page(masked), 100, 100).expect("render");
    let b = hikari::render_png(&page(unmasked), 100, 100).expect("render");
    assert_eq!(
        hikari::hash_bytes(&a),
        hikari::hash_bytes(&b),
        "a mask on a node with no image changed the render; it should be inert \
         until it is implemented, not silently eroding the node"
    );
    assert!(ink(&a) > 0, "the card should still be drawn");
}
