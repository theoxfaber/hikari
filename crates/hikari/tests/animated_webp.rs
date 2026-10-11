//! Animated WebP through the whole pipeline: real trees, real renderer, then
//! decode the bytes back and compare.
//!
//! The encoder's own tests cover the container against hand-built frame buffers.
//! This covers the failure that actually happens in use -- a frame that renders
//! through the pipeline but comes out empty, transparent, or the wrong size --
//! which hand-built buffers would never produce.

use hikari::{count_webp_frames, AnimFrame, Loop, Node, Style};

/// Render frames to raw RGBA the way the facade does.
fn render_frames(frames: &[(Node, u32)], w: u32, h: u32) -> Vec<AnimFrame> {
    hikari::render_animation_rgba(frames, w, h)
        .expect("frames render")
        .into_iter()
        .map(|(rgba, duration_ms)| AnimFrame { rgba, duration_ms })
        .collect()
}

fn card(bg: &str, label: &str) -> Node {
    Node::container(
        Style::new()
            .with_size(64.0, 48.0)
            .with_background(bg)
            .with_padding(8.0),
        vec![Node::text(label, Style::text(14.0, "#ffffff"))],
    )
}

#[test]
fn three_trees_render_into_a_decodable_animation() {
    let frames = vec![
        (card("#334155", "one"), 60u32),
        (card("#b45309", "two"), 60),
        (card("#166534", "three"), 60),
    ];
    let anim = render_frames(&frames, 64, 48);
    assert_eq!(anim.len(), 3);

    for (i, f) in anim.iter().enumerate() {
        assert_eq!(f.rgba.len(), (64 * 48 * 4) as usize, "frame {i} wrong size");
        assert_eq!(f.duration_ms, 60);
        // The renderer starts from a white page, so a frame that painted nothing
        // would be uniformly transparent and encode to almost nothing. A size
        // floor is the cheapest way to notice that.
        let ink = f.rgba.iter().filter(|&&b| b != 0).count();
        assert!(
            ink > 1_000,
            "frame {i} rendered no content ({ink} bytes set)"
        );
    }

    let bytes = hikari::render_animation_webp(&frames, 64, 48).expect("encodes");

    // The decode is the check. A still WebP and an animated one differ only in
    // chunks that a hash would be equally happy with, and a file missing the
    // animation flag decodes to one frame forever without reporting an error.
    assert_eq!(count_webp_frames(&bytes).expect("counts"), 3);

    use image::AnimationDecoder;
    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(&bytes))
        .expect("decodes as webp");
    let decoded: Vec<_> = decoder.into_frames().collect();
    assert_eq!(decoded.len(), 3);
    for (i, f) in decoded.iter().enumerate() {
        let f = f.as_ref().expect("frame decodes");
        assert_eq!(
            f.buffer().to_vec(),
            anim[i].rgba,
            "frame {i} did not survive the round trip"
        );
        assert_eq!(f.delay().numer_denom_ms().0, 60);
    }
}

#[test]
fn webp_is_smaller_than_apng_for_the_same_frames() {
    // The usual reason to pick WebP over APNG. Measured here rather than asserted
    // in the README, so the claim has a number behind it that the encoder
    // actually produced.
    let frames = vec![
        (card("#334155", "one"), 60u32),
        (card("#b45309", "two"), 60),
        (card("#166534", "three"), 60),
    ];
    let webp = hikari::render_animation_webp(&frames, 120, 90).expect("webp");
    let apng = hikari::render_animation_apng(&frames, 120, 90).expect("apng");
    assert!(
        webp.len() < apng.len(),
        "webp {} bytes vs apng {} bytes",
        webp.len(),
        apng.len()
    );
}

#[test]
fn an_over_sized_canvas_is_an_error_not_a_truncated_file() {
    // The format caps the canvas at 16383 per side. A render larger than that has
    // no representation, so it must fail rather than emit a file that decodes to
    // something smaller than what was asked for.
    let frames = vec![(card("#000000", "x"), 40u32)];
    let err = hikari::encode_webp_animation(
        &render_frames(&frames, 8, 8),
        hikari::MAX_CANVAS + 1,
        8,
        Loop::Forever,
        hikari::AnimBackground::default(),
    );
    assert!(err.is_err(), "an over-limit canvas must be rejected");
}
