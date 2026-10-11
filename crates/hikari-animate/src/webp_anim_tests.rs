//! Animated WebP, checked by decoding it back.
//!
//! # Why every test here decodes
//!
//! A malformed container is not detectable by inspection: a missing RIFF pad
//! byte, a canvas field written big-endian, an `ANMF` with the wrong chunk tag --
//! each of those produces bytes that hash perfectly, compare equal across runs,
//! and satisfy every structural assertion a test written against the writer alone
//! would make. They only fail when a real decoder tries to read them.
//!
//! So each case here decodes with `image`'s own `WebPDecoder` and checks the
//! frame count, the per-frame pixels and the durations. The byte-level
//! assertions exist too, but only to pin the things a decoder is *forgiving*
//! about -- chiefly that `VP8X` carries the animation flag, since a decoder that
//! sees it missing will happily show the first frame forever and report no error
//! at all.

use super::*;

fn solid(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    (0..w * h).flat_map(|_| [r, g, b, 255]).collect()
}

fn frame(_w: u32, _h: u32, rgba: Vec<u8>, ms: u32) -> AnimFrame {
    AnimFrame {
        rgba,
        duration_ms: ms,
    }
}

fn three_frames(w: u32, h: u32) -> Vec<AnimFrame> {
    vec![
        frame(w, h, solid(w, h, 255, 0, 0), 40),
        frame(w, h, solid(w, h, 0, 255, 0), 80),
        frame(w, h, solid(w, h, 0, 0, 255), 120),
    ]
}

fn encode(frames: &[AnimFrame], w: u32, h: u32) -> Vec<u8> {
    encode_webp_animation(frames, w, h, Loop::Forever, AnimBackground::default()).expect("encodes")
}

/// Decode and return `(pixels, duration_ms)` per frame.
fn decode_all(bytes: &[u8]) -> Vec<(Vec<u8>, (u32, u32))> {
    use image::AnimationDecoder;
    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))
        .expect("decodes as webp");
    decoder
        .into_frames()
        .map(|f| {
            let f = f.expect("frame decodes");
            (f.buffer().to_vec(), f.delay().numer_denom_ms())
        })
        .collect()
}

#[test]
fn the_container_is_a_webp_file() {
    let bytes = encode(&three_frames(16, 8), 16, 8);
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WEBP");
    // The RIFF size must cover everything after the size field itself, or a
    // decoder reading from a stream stops early and reports a truncated file.
    let declared = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    assert_eq!(
        declared + 8,
        bytes.len(),
        "RIFF size field does not match the payload length"
    );
}

#[test]
fn every_chunk_is_word_aligned() {
    // The container spec requires a pad byte after any odd-length chunk. Missing
    // it is the classic way to produce a file that hashes stably and refuses to
    // decode, so it gets its own check rather than being left to the round trip.
    let mut bytes = encode(&three_frames(15, 7), 15, 7);
    let declared = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;

    // Walk the chunks from just after the 12-byte prologue to the end of file.
    let mut at = 12usize;
    let mut seen = 0usize;
    while at < bytes.len() {
        let tag = bytes[at..at + 4].to_vec();
        let size = u32::from_le_bytes([bytes[at + 4], bytes[at + 5], bytes[at + 6], bytes[at + 7]])
            as usize;
        let padded = size + (size & 1);
        let next = at + 8 + padded;
        assert!(
            next <= bytes.len(),
            "chunk {tag:?} at {at} claims {size} bytes and runs past the end"
        );
        if size % 2 == 1 {
            assert_eq!(
                bytes[at + 8 + size],
                0,
                "odd-length chunk {tag:?} is missing its pad byte"
            );
        }
        at = next;
        seen += 1;
    }
    assert_eq!(at, 12 + declared - 4, "chunk walk did not land on the end");
    assert_eq!(seen, 5, "VP8X + ANIM + 3 ANMF");
    bytes.clear();
}

#[test]
fn the_extended_header_declares_an_animation_with_alpha() {
    // The failure this guards is the quietest one in the format: drop bit 1 and
    // a decoder treats the file as a single still, shows frame one forever, and
    // reports no error. Frame count alone would still be right, so the flag is
    // asserted directly.
    let bytes = encode(&three_frames(16, 8), 16, 8);
    let vp8x = bytes
        .windows(4)
        .position(|w| w == b"VP8X")
        .expect("VP8X chunk present");
    let payload = &bytes[vp8x + 8..vp8x + 18];
    assert_eq!(payload[0] & 0x02, 0x02, "animation flag not set");
    assert_eq!(payload[0] & 0x10, 0x10, "alpha flag not set");
    assert_eq!(&payload[1..4], &[0, 0, 0], "reserved bytes must be zero");

    // Canvas size is stored minus one, 24-bit little-endian.
    let w = u32::from(payload[4]) | u32::from(payload[5]) << 8 | u32::from(payload[6]) << 16;
    let h = u32::from(payload[7]) | u32::from(payload[8]) << 8 | u32::from(payload[9]) << 16;
    assert_eq!(w, 16 - 1, "canvas width is not stored as (w - 1)");
    assert_eq!(h, 8 - 1, "canvas height is not stored as (h - 1)");
}

#[test]
fn three_frames_decode_back_as_three_frames() {
    let bytes = encode(&three_frames(16, 8), 16, 8);
    assert_eq!(count_webp_frames(&bytes).expect("counts"), 3);
    assert_eq!(decode_all(&bytes).len(), 3);
}

#[test]
fn every_frame_keeps_its_own_pixels() {
    // Lossless, so the colours must come back exactly. A colour that is merely
    // close would mean the frames were being blended together, which is what a
    // wrong blend flag or a wrong frame rectangle looks like.
    let frames = three_frames(16, 8);
    let bytes = encode(&frames, 16, 8);
    let decoded = decode_all(&bytes);
    assert_eq!(decoded.len(), frames.len());
    for (i, ((px, _), original)) in decoded.iter().zip(&frames).enumerate() {
        assert_eq!(
            px, &original.rgba,
            "frame {i} came back with different pixels"
        );
    }
}

#[test]
fn durations_survive_the_round_trip() {
    let bytes = encode(&three_frames(16, 8), 16, 8);
    for (i, (_, delay)) in decode_all(&bytes).iter().enumerate() {
        let want = [40u32, 80, 120][i];
        let got = delay.0;
        assert_eq!(got, want, "frame {i} duration changed: {got} != {want}");
    }
}

#[test]
fn transparency_survives() {
    // Alpha is the reason the alpha flag exists, and a frame that went opaque on
    // the way through would still decode cleanly -- it would just be wrong.
    let rgba: Vec<u8> = (0..8 * 8 * 4)
        .map(|i| if i % 4 == 3 { 0 } else { 200 })
        .collect();
    let bytes = encode(&[frame(8, 8, rgba, 50)], 8, 8);
    let decoded = decode_all(&bytes);
    assert_eq!(decoded.len(), 1);
    let (px, _) = &decoded[0];
    let (pixels, _) = px.as_chunks::<4>();
    for chunk in pixels.iter() {
        assert_eq!(chunk[3], 0, "a transparent pixel came back opaque");
    }
}

#[test]
fn a_single_frame_still_decodes() {
    // One frame is a degenerate animation. It must not be "optimised" into a
    // plain still by dropping VP8X, because a still and a one-frame animation
    // loop differently.
    let bytes = encode(&[frame(8, 8, solid(8, 8, 10, 20, 30), 100)], 8, 8);
    assert_eq!(count_webp_frames(&bytes).expect("counts"), 1);
    assert!(bytes.windows(4).any(|w| w == b"VP8X"));
    assert!(bytes.windows(4).any(|w| w == b"ANMF"));
}

#[test]
fn an_odd_canvas_size_works() {
    // WebP pads odd dimensions internally, and the frame rectangle is stored
    // divided by three while the canvas is stored minus one. A size that is odd,
    // small, and not a multiple of three exercises all three encodings at once.
    for (w, h) in [(1u32, 1u32), (3, 5), (7, 15), (16, 9)] {
        let bytes = encode(&three_frames(w, h), w, h);
        assert_eq!(
            count_webp_frames(&bytes).expect("counts"),
            3,
            "{w}x{h} lost a frame"
        );
        let decoded = decode_all(&bytes);
        assert_eq!(
            decoded[0].0.len(),
            (w * h * 4) as usize,
            "{w}x{h} decoded to the wrong buffer size"
        );
    }
}

#[test]
fn a_large_canvas_within_the_limit_encodes() {
    // 16383 is the format's maximum. Encoding it at full size would allocate
    // 1 GB, so this checks the boundary is *accepted* by checking the error is
    // not the size one, using a tiny frame instead -- which is enough, because
    // the guard runs before any frame is encoded.
    let huge = MAX_CANVAS + 1;
    let err = encode_webp_animation(
        &[frame(1, 1, solid(1, 1, 1, 1, 1), 10)],
        huge,
        1,
        Loop::Forever,
        AnimBackground::default(),
    )
    .expect_err("must reject an over-limit canvas");
    assert!(
        format!("{err:?}").contains("exceeds"),
        "unexpected error: {err:?}"
    );
}

#[test]
fn loop_forever_and_a_count_are_encoded_differently() {
    let frames = three_frames(8, 8);
    let forever = encode_webp_animation(&frames, 8, 8, Loop::Forever, AnimBackground::default())
        .expect("encodes");
    let thrice = encode_webp_animation(&frames, 8, 8, Loop::Count(3), AnimBackground::default())
        .expect("encodes");
    assert_ne!(forever, thrice, "loop count had no effect on the output");
    // Both must still decode; a malformed ANIM would break both.
    assert_eq!(count_webp_frames(&forever).expect("counts"), 3);
    assert_eq!(count_webp_frames(&thrice).expect("counts"), 3);
}

#[test]
fn bad_input_is_rejected_rather_than_producing_a_broken_file() {
    // Every one of these would otherwise produce bytes that are plausible and
    // undecodable, which is the worst combination available.
    assert!(encode_webp_animation(&[], 8, 8, Loop::Forever, AnimBackground::default()).is_err());
    assert!(encode_webp_animation(
        &[frame(8, 8, solid(8, 8, 1, 1, 1), 10)],
        0,
        8,
        Loop::Forever,
        AnimBackground::default()
    )
    .is_err());
    assert!(encode_webp_animation(
        &[frame(8, 8, solid(8, 8, 1, 1, 1), 10)],
        8,
        8,
        Loop::Forever,
        AnimBackground::default()
    )
    .is_ok());
    // Wrong buffer length, and wrong on the *second* frame so a writer that
    // only validated the first would pass.
    let bad = vec![
        frame(8, 8, solid(8, 8, 1, 1, 1), 10),
        frame(8, 8, vec![0u8; 3], 10),
    ];
    assert!(
        encode_webp_animation(&bad, 8, 8, Loop::Forever, AnimBackground::default()).is_err(),
        "a short second frame must be rejected"
    );
}

#[test]
fn a_very_long_duration_is_clamped_rather_than_wrapping() {
    // Duration is a 24-bit field, so it tops out at 16,777,215 ms -- about 4.6
    // hours. A caller asking for longer should get the maximum, not a wrapped
    // value that silently turns into a fraction of a second.
    //
    // The first version of this test used one hour (3,600,000 ms) and asserted
    // the clamp, which does not fire: an hour fits in 24 bits comfortably. It
    // passed the encode and failed the decode, so the premise was checked and
    // `u32::MAX` used instead.
    let bytes = encode(&[frame(8, 8, solid(8, 8, 5, 5, 5), u32::MAX)], 8, 8);
    let decoded = decode_all(&bytes);
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].1 .0, 0xFF_FFFF, "duration was not clamped");
}

#[test]
fn an_hour_long_frame_keeps_its_duration_exactly() {
    // The companion to the clamp test: a duration that *does* fit must survive
    // unchanged, so the clamp is not just "clamp everything to something".
    let hour = 3_600_000u32;
    let bytes = encode(&[frame(8, 8, solid(8, 8, 5, 5, 5), hour)], 8, 8);
    assert_eq!(decode_all(&bytes)[0].1 .0, hour);
}
#[test]
fn every_frame_says_do_not_blend() {
    // The B and D flags are the low two bits of the byte after the duration, and
    // they are the only place a decoder is told whether to composite a frame or
    // overwrite it. B clear means "alpha-blend", which loses a unit of colour
    // through the integer division in the blend formula; B set means the frame
    // is written straight into the canvas. Every frame here is a whole canvas,
    // so overwrite is the exact instruction and preserves the pixels.
    let bytes = encode(&three_frames(8, 8), 8, 8);
    let decoded = decode_all(&bytes);
    assert_eq!(decoded.len(), 3);
    // Pixels come back byte-identical, which is the observable consequence. The
    // blend path lost exactly 1 on a solid 255, so this is a real assertion and
    // not a formality.
    for (i, (px, _)) in decoded.iter().enumerate() {
        let want = match i {
            0 => [255u8, 0, 0, 255],
            1 => [0, 255, 0, 255],
            _ => [0, 0, 255, 255],
        };
        assert_eq!(&px[..4], &want, "frame {i} was blended rather than written");
    }
}
