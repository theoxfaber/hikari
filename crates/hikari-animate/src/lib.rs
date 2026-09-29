#![warn(missing_docs)]
//! Animated encoders (Pro): straight-alpha RGBA frames -> GIF bytes.
//!
//! Frames are rendered in parallel by the facade and encoded here.
//! APNG/WebP encoders are next; GIF ships first because the whole path is
//! pure Rust with no system dependencies.

use hikari_core::Error;
use image::codecs::gif::{GifEncoder, Repeat};
use image::{Delay, Frame, RgbaImage};

/// One animation frame: straight-alpha RGBA + display time.
#[derive(Debug, Clone)]
pub struct AnimFrame {
    /// Straight-alpha RGBA pixels, `w*h*4`.
    pub rgba: Vec<u8>,
    /// Display duration in ms.
    pub duration_ms: u32,
}

/// Encode `frames` (all `w`x`h`) as an infinite-loop GIF.
pub fn encode_gif(frames: &[AnimFrame], w: u32, h: u32) -> Result<Vec<u8>, Error> {
    if frames.is_empty() {
        return Err(Error::Encode("no frames".into()));
    }
    if w == 0 || h == 0 {
        return Err(Error::Encode("zero-size canvas".into()));
    }
    let expect = (w * h * 4) as usize;
    let mut out = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut out);
        enc.set_repeat(Repeat::Infinite)
            .map_err(|e| Error::Encode(e.to_string()))?;
        for f in frames {
            if f.rgba.len() != expect {
                return Err(Error::Encode(format!(
                    "frame size {} != canvas {expect}",
                    f.rgba.len()
                )));
            }
            let buf = RgbaImage::from_raw(w, h, f.rgba.clone())
                .ok_or_else(|| Error::Encode("frame buffer invalid".into()))?;
            let frame = Frame::from_parts(
                buf,
                0,
                0,
                Delay::from_numer_denom_ms(f.duration_ms.max(20), 1),
            );
            enc.encode_frame(frame)
                .map_err(|e| Error::Encode(e.to_string()))?;
        }
    }
    Ok(out)
}

/// Count frames in GIF bytes (decoder check used by tests).
pub fn count_gif_frames(bytes: &[u8]) -> Result<usize, Error> {
    use image::AnimationDecoder;
    use std::io::Cursor;
    let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .map_err(|e| Error::Asset(e.to_string()))?;
    Ok(decoder.into_frames().count())
}

/// Encode `frames` (all `w`x`h` straight-alpha RGBA) as a looping APNG.
pub fn encode_apng(frames: &[AnimFrame], w: u32, h: u32) -> Result<Vec<u8>, Error> {
    use png::{BitDepth, ColorType, Encoder};
    if frames.is_empty() {
        return Err(Error::Encode("no frames".into()));
    }
    if w == 0 || h == 0 {
        return Err(Error::Encode("zero-size canvas".into()));
    }
    let expect = (w * h * 4) as usize;
    let mut out = Vec::new();
    {
        let mut enc = Encoder::new(&mut out, w, h);
        enc.set_color(ColorType::Rgba);
        enc.set_depth(BitDepth::Eight);
        enc.set_animated(frames.len() as u32, 0)
            .map_err(|e| Error::Encode(e.to_string()))?;
        let mut writer = enc
            .write_header()
            .map_err(|e| Error::Encode(e.to_string()))?;
        for f in frames {
            if f.rgba.len() != expect {
                return Err(Error::Encode(format!(
                    "frame size {} != canvas {expect}",
                    f.rgba.len()
                )));
            }
            writer
                .set_frame_delay(f.duration_ms.min(60_000) as u16, 1000)
                .map_err(|e| Error::Encode(e.to_string()))?;
            writer
                .write_image_data(&f.rgba)
                .map_err(|e| Error::Encode(e.to_string()))?;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
        (0..w * h).flat_map(|_| [r, g, b, 255]).collect()
    }

    #[test]
    fn gif_roundtrip_two_frames() {
        let frames = vec![
            AnimFrame {
                rgba: solid(32, 16, 200, 30, 30),
                duration_ms: 500,
            },
            AnimFrame {
                rgba: solid(32, 16, 30, 60, 200),
                duration_ms: 500,
            },
        ];
        let bytes = encode_gif(&frames, 32, 16).unwrap();
        assert_eq!(&bytes[..6], b"GIF89a");
        assert_eq!(count_gif_frames(&bytes).unwrap(), 2);
    }

    #[test]
    fn apng_roundtrip_two_frames() {
        let frames = vec![
            AnimFrame {
                rgba: solid(32, 16, 200, 30, 30),
                duration_ms: 500,
            },
            AnimFrame {
                rgba: solid(32, 16, 30, 60, 200),
                duration_ms: 500,
            },
        ];
        let bytes = encode_apng(&frames, 32, 16).unwrap();
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        assert!(
            bytes.windows(4).any(|w| w == b"acTL"),
            "APNG animation chunk"
        );
        assert!(bytes.len() > 200);
    }

    #[test]
    fn gif_rejects_bad_input() {
        assert!(encode_gif(&[], 32, 16).is_err());
        let bad = vec![AnimFrame {
            rgba: vec![0u8; 10],
            duration_ms: 100,
        }];
        assert!(encode_gif(&bad, 32, 16).is_err());
    }
}
