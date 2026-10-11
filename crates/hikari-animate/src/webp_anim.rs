//! Animated WebP encoding.
//!
//! # Why this is hand-rolled
//!
//! `image-webp` has no animation support in either direction -- no encoder for
//! `ANIM`/`ANMF` and no `AnimationDecoder` of its own -- so there is nothing to
//! call. It is also still-only-lossless, which happens to be exactly what an
//! animated file needs, since a lossy VP8 frame carries alpha only through a
//! separate `ALPH` chunk and this crate does not want to pair one with a
//! container it is also writing by hand.
//!
//! So the container is written here and the frame *bitstreams* are delegated to
//! `image`'s still encoder. That split is the whole trick and it is safe for a
//! specific reason worth stating, because it is what makes it safe:
//! `WebPEncoder::new_lossless` emits exactly one `VP8L ` chunk after the 12-byte
//! `RIFF`-size-`WEBP` prologue. Dropping that prologue leaves a valid chunk, and
//! dropping anything else would not. A test asserts the prologue's exact shape so
//! a future `image` release that adds, say, an `EXIF` chunk cannot silently turn
//! this into a file that decodes to one frame.
//!
//! # Layout
//!
//! ```text
//! RIFF <size> WEBP
//!   VP8X <10>  flags, canvas w-1, canvas h-1
//!   ANIM <6>   background BGRA, loop count
//!   ANMF <n>   16 bytes of frame geometry, then the frame bitstream
//!   ANMF ...
//! ```
//!
//! Every chunk payload is padded to an even length with one zero byte, which the
//! RIFF container requires and which a decoder rejects the file over if missed.
//!
//! Two details of `ANMF` are easy to get backwards, and both were:
//!
//! * The 16 bytes of geometry come **before** the frame bitstream, not after.
//! * `Frame X` and `Frame Y` are the coordinate **divided by 2**, not by 3.
//!
//! Both were written the other way round here first and produced a file that
//! hashed consistently, passed every structural check, and decoded to a single
//! frame of the wrong size. The spec diagram above is the source; the decoder
//! round trip is what catches it.

use hikari_core::Error;

use super::AnimFrame;

/// Largest canvas WebP supports, per the container specification.
///
/// Not an arbitrary cap: the `VP8X` canvas fields are 24-bit *and* the format
/// fixes the limit at 2^14 - 1. A larger canvas has no representation, so a
/// request for one is an error rather than a truncated file.
pub const MAX_CANVAS: u32 = 16_383;

/// How many times an animation repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loop {
    /// Forever. Stored as `0`, which is what the format means by it.
    Forever,
    /// A fixed count. Stored as-is; the spec treats `0` as forever, so a literal
    /// zero here is clamped to one repeat rather than silently becoming infinite.
    Count(u16),
}

impl Loop {
    fn encoded(self) -> u16 {
        match self {
            Loop::Forever => 0,
            Loop::Count(0) => 1,
            Loop::Count(n) => n,
        }
    }
}

/// Background colour shown where no frame has been drawn.
///
/// Straight RGBA, matching [`AnimFrame`]'s convention. The container stores it
/// premultiplied as BGRA, so the two are swapped *and* multiplied here rather
/// than at the call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnimBackground(pub u8, pub u8, pub u8, pub u8);

impl Default for AnimBackground {
    fn default() -> Self {
        // Opaque white: the same canvas the renderer starts from, so a frame with
        // transparent edges blends onto what the caller would have seen anyway.
        Self(0xFF, 0xFF, 0xFF, 0xFF)
    }
}

/// Encode straight-alpha RGBA frames as an animated WebP.
///
/// Every frame is written at the full canvas with no cropping and no
/// frame-to-frame optimisation, which is a real size cost on animations that
/// change only in one corner -- the optimisation WebP's own muxer performs --
/// but it is also what makes each frame independently decodable, so a broken
/// frame costs one frame rather than the rest of the animation.
///
/// Frames need not be opaque: the format carries alpha per frame and `VP8X` is
/// flagged for it.
pub fn encode_webp_animation(
    frames: &[AnimFrame],
    w: u32,
    h: u32,
    loop_count: Loop,
    background: AnimBackground,
) -> Result<Vec<u8>, Error> {
    if frames.is_empty() {
        return Err(Error::Encode("no frames".into()));
    }
    if w == 0 || h == 0 {
        return Err(Error::Encode("zero-size canvas".into()));
    }
    if w > MAX_CANVAS || h > MAX_CANVAS {
        return Err(Error::Encode(format!(
            "canvas {w}x{h} exceeds the WebP limit of {MAX_CANVAS}"
        )));
    }
    let expect = (w as usize) * (h as usize) * 4;

    // Body = every chunk after the 12-byte RIFF prologue, built first so the
    // prologue's size field can be derived from it rather than guessed and
    // patched. RIFF sizes cover everything after the size field itself.
    let mut body: Vec<u8> = Vec::new();

    // VP8X: flag byte, 3 reserved, then canvas-1 as 24-bit LE each.
    // Bit 1 (0x02) is "animation" and bit 4 (0x10) is "alpha". Both are set:
    // without the animation flag a decoder reads this as a single still image and
    // shows only the first frame.
    let mut vp8x = Vec::with_capacity(10);
    vp8x.push(0x02 | 0x10);
    vp8x.extend_from_slice(&[0, 0, 0]);
    put_u24(&mut vp8x, w - 1);
    put_u24(&mut vp8x, h - 1);
    write_chunk(&mut body, b"VP8X", &vp8x);

    // ANIM: BGRA background, then a 16-bit loop count.
    let AnimBackground(r, g, b, a) = background;
    let mut anim = Vec::with_capacity(6);
    anim.extend_from_slice(&[
        (b as u16 * a as u16 / 255) as u8,
        (g as u16 * a as u16 / 255) as u8,
        (r as u16 * a as u16 / 255) as u8,
        a,
    ]);
    anim.extend_from_slice(&loop_count.encoded().to_le_bytes());
    write_chunk(&mut body, b"ANIM", &anim);

    for f in frames {
        if f.rgba.len() != expect {
            return Err(Error::Encode(format!(
                "frame size {} != canvas {expect}",
                f.rgba.len()
            )));
        }
        let mut payload = Vec::with_capacity(16 + 64);
        // Frame geometry, in the order the specification's `ANMF` diagram
        // gives: X, Y, width-1, height-1, duration, then one byte holding
        // 6 reserved bits and the B and D flags.
        //
        // X and Y are stored as the coordinate divided by 2, so every origin
        // this encoder can express is even. Full-canvas frames are at (0, 0), so
        // that constraint never binds here -- but it is the reason the field is
        // scaled at all, and getting the divisor wrong decodes as a frame of
        // one-and-a-half times the intended size.
        put_u24(&mut payload, 0);
        put_u24(&mut payload, 0);
        put_u24(&mut payload, w - 1);
        put_u24(&mut payload, h - 1);
        put_u24(&mut payload, f.duration_ms.min(0xFF_FFFF));
        // B (blend) and D (dispose) are the last two bits, B above D.
        //
        // B is set to 1, "do not blend", because every frame this encoder writes
        // is a complete canvas and overwriting is exactly what that means.
        // Alpha-blending would also be defensible for full frames, but it is not
        // free: the blend the specification defines recomputes colour through
        // `(src*A + dst*A*(1 - A)) / blendA`, which loses a unit on the way
        // through integer division -- a solid red frame came back as 254, 0, 0.
        // Overwriting preserves the frame byte for byte, which is what a lossless
        // encoder should do and what makes the round trip worth asserting on
        // exactly rather than to within a tolerance.
        //
        // D stays 0, "do not dispose": the canvas is left as is between frames.
        // With full-canvas frames the next frame replaces every pixel anyway, so
        // disposal could not make a difference -- and leaving it clear is what
        // other muxers write for simple animations.
        payload.push(0x02);
        payload.extend_from_slice(&still_bitstream(&f.rgba, w, h)?);
        write_chunk(&mut body, b"ANMF", &payload);
    }

    let mut out = Vec::with_capacity(body.len() + 12);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
    out.extend_from_slice(b"WEBP");
    out.extend_from_slice(&body);
    Ok(out)
}

/// Encode one frame's pixels as a bare `VP8L ` chunk.
///
/// Strips the 12-byte `RIFF`-size-`WEBP` prologue from `image`'s still encoder.
/// That strip is only correct because the encoder emits exactly one chunk; that
/// is asserted rather than assumed.
fn still_bitstream(rgba: &[u8], w: u32, h: u32) -> Result<Vec<u8>, Error> {
    use image::{ExtendedColorType, ImageEncoder};

    let mut still = Vec::new();
    image::codecs::webp::WebPEncoder::new_lossless(&mut still)
        .write_image(rgba, w, h, ExtendedColorType::Rgba8)
        .map_err(|e| Error::Encode(format!("webp frame encode failed: {e}")))?;

    const PROLOGUE: usize = 12;
    if still.len() <= PROLOGUE || &still[..4] != b"RIFF" || &still[8..12] != b"WEBP" {
        return Err(Error::Encode(
            "webp still encoder did not emit a plain RIFF/WEBP file".into(),
        ));
    }
    // Anything between the prologue and the first chunk would be metadata this
    // crate does not understand, and carrying it into an ANMF payload would
    // produce a frame a decoder rejects.
    if &still[PROLOGUE..PROLOGUE + 4] != b"VP8L" {
        return Err(Error::Encode(
            "webp still encoder emitted something other than a VP8L chunk".into(),
        ));
    }
    let payload_len = u32::from_le_bytes([still[16], still[17], still[18], still[19]]) as usize;
    // 8 header bytes, the payload, and one pad byte if the payload is odd.
    let expected = PROLOGUE + 8 + payload_len + (payload_len & 1);
    if still.len() != expected {
        return Err(Error::Encode(format!(
            "webp still encoder emitted {} bytes, expected {expected} for a single VP8L chunk",
            still.len()
        )));
    }
    Ok(still[PROLOGUE..].to_vec())
}

/// Append a RIFF chunk: 4-byte tag, 32-bit LE payload size, payload, pad.
fn write_chunk(out: &mut Vec<u8>, tag: &[u8; 4], payload: &[u8]) {
    out.extend_from_slice(tag);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        // RIFF chunks are word-aligned. The container spec requires the pad byte
        // and decoders reject a file that omits it, so it is written even when
        // the payload's own trailing byte looks like padding.
        out.push(0);
    }
}

/// Append a 24-bit little-endian value.
fn put_u24(out: &mut Vec<u8>, v: u32) {
    out.push((v & 0xFF) as u8);
    out.push(((v >> 8) & 0xFF) as u8);
    out.push(((v >> 16) & 0xFF) as u8);
}

/// Count frames in animated WebP bytes.
///
/// Present for the same reason as [`super::count_gif_frames`]: a decoder round
/// trip is the only check of an animated container that proves anything, since a
/// malformed one still hashes perfectly.
pub fn count_webp_frames(bytes: &[u8]) -> Result<usize, Error> {
    use image::AnimationDecoder;
    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Asset(format!("not a decodable WebP: {e}")))?;
    Ok(decoder.into_frames().count())
}
