#![warn(missing_docs)]
//! `hikari-core`: node tree, styles, `taffy` layout, content-hash cache.
//!
//! Intentionally small: containers + text only. This covers most OG cards
//! while keeping layout predictable and the dependency graph upstream-only.

mod cache;
mod error;
mod flow;
mod font;
mod shape;

pub use font::{
    builtin_bytes, font_entry, hb_face, register_font, registered_font_count, FontEntry, FontId,
    BUILTIN_FONT,
};

/// A CSS subset parser and an HTML-ish front end.
pub mod assets;
pub mod css;
pub mod policy;
pub mod spec;
pub mod ui;

// Both modules below contain only `#[test]` items, so compiling them in a
// non-test build strips every function and leaves only the helpers and imports
// behind, which then read as dead code. Gating the module is the honest fix;
// gating the individual items just moves the same warning down the file.
#[cfg(test)]
mod css_tests;
#[cfg(test)]
mod filter_tests;
#[cfg(test)]
mod font_chain_tests;
#[cfg(test)]
mod font_subset_tests;
#[cfg(test)]
mod shaping_path_tests;

pub use cache::{hash_bytes, hash_node, HashCache};
pub use error::Error;
pub use flow::{paginate, Flow};
pub use font::glyph_index_in;
pub use policy::{fallback_policy, set_fallback_policy, FallbackPolicy};
pub use shape::{
    balance_text, fallback_font_bytes, fit_font_size, font_bytes, line_height, measure_text,
    shape_text, wrap_text, PlacedAdvance,
};

use serde::{Deserialize, Serialize};
use taffy::{
    geometry::Size,
    style::{
        AlignItems, Dimension, FlexDirection, JustifyContent, LengthPercentage, Style as TaffyStyle,
    },
    AvailableSpace, TaffyTree,
};

/// RGBA color.
///
/// Deserializes from either a CSS hex string or an `{r,g,b,a}` map. The string
/// form exists because requiring `{"r":255,"g":255,"b":255,"a":255}` in the
/// Node and WASM bindings makes the most common field in the API the most
/// annoying one to write. Both forms are accepted; the struct form is what
/// gets serialized, so output is unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Color {
    /// Red 0-255.
    pub r: u8,
    /// Green 0-255.
    pub g: u8,
    /// Blue 0-255.
    pub b: u8,
    /// Alpha 0-255.
    pub a: u8,
}

impl Color {
    /// Opaque RGB.
    #[must_use]
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    /// Parse `#rgb`, `#rrggbb`, `#rrggbbaa`. Returns black on invalid input.
    #[must_use]
    pub fn from_hex(s: &str) -> Self {
        let h = s.trim_start_matches('#');
        let parse = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0);
        match h.len() {
            3 => {
                let e = |c: char| u8::from_str_radix(&format!("{c}{c}"), 16).unwrap_or(0);
                let mut c = h.chars();
                Self::rgb(
                    e(c.next().unwrap_or('0')),
                    e(c.next().unwrap_or('0')),
                    e(c.next().unwrap_or('0')),
                )
            }
            6 => Self::rgb(parse(0), parse(2), parse(4)),
            8 => Self {
                r: parse(0),
                g: parse(2),
                b: parse(4),
                a: parse(6),
            },
            _ => Self::rgb(0, 0, 0),
        }
    }

    /// Convert to `tiny-skia`-compatible floats without depending on it here.
    #[must_use]
    pub fn to_rgba_f32(self) -> (f32, f32, f32, f32) {
        (
            f32::from(self.r) / 255.0,
            f32::from(self.g) / 255.0,
            f32::from(self.b) / 255.0,
            f32::from(self.a) / 255.0,
        )
    }
}

/// Accepts `"#rrggbb"`, `"#rgb"`, `"#rrggbbaa"`, a bare hex string without the
/// `#`, or `{"r":…,"g":…,"b":…,"a":…}` (alpha optional, defaults to opaque).
impl<'de> Deserialize<'de> for Color {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Hex(String),
            Rgba {
                r: u8,
                g: u8,
                b: u8,
                #[serde(default = "opaque")]
                a: u8,
            },
        }
        fn opaque() -> u8 {
            255
        }
        Ok(match Repr::deserialize(d)? {
            Repr::Hex(s) => Self::from_hex(&s),
            Repr::Rgba { r, g, b, a } => Self { r, g, b, a },
        })
    }
}

/// Main-axis direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FlexDir {
    /// Horizontal.
    #[default]
    Row,
    /// Vertical.
    Column,
}

/// Main-axis alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Justify {
    /// Pack at start.
    #[default]
    Start,
    /// Pack at center.
    Center,
    /// Pack at end.
    End,
    /// Even space between.
    SpaceBetween,
    /// Even space around.
    SpaceAround,
}

/// Cross-axis alignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Align {
    /// Pack at start.
    #[default]
    Start,
    /// Pack at center.
    Center,
    /// Pack at end.
    End,
    /// Stretch to fill.
    Stretch,
}

/// Layout algorithm for a container's children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Display {
    /// Flexbox row/column (uses `dir`).
    #[default]
    Flex,
    /// Equal-width column grid (uses `grid_cols`).
    Grid,
    /// Block stacking.
    Block,
}

/// Box shadow: offset blurred silhouette behind the box.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Shadow {
    /// Which edge casts it, and therefore whether it lands behind or inside the
    /// box. Defaults to [`ShadowKind::Drop`] so a tree serialized before v0.20
    /// deserializes to the behaviour it rendered with then.
    #[serde(default)]
    pub kind: ShadowKind,
    /// Horizontal offset in px.
    pub dx: f32,
    /// Vertical offset in px.
    pub dy: f32,
    /// Blur radius in px (`0` = sharp offset).
    pub blur: f32,
    /// Grow the silhouette in px before blurring.
    pub spread: f32,
    /// Shadow color (alpha respected).
    pub color: Color,
}

/// How a node's content combines with what is already painted beneath it.
///
/// Defaults to [`BlendMode::Normal`], which is what every render did before
/// this existed, so a tree that names no blend mode is byte-identical. The
/// separable blend modes are named after the Porter-Duff/Photoshop set they
/// implement rather than their CSS spelling, because the CSS names are ambiguous
/// in prose ("multiply" is both a mode and an operation) and the SVG `mix-blend-mode`
/// values are the ones a reader will look up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BlendMode {
    /// Source over destination. The default, and ordinary alpha compositing.
    #[default]
    Normal,
    /// Multiply, which darkens where both are dark.
    Multiply,
    /// Screen, the inverse of multiply.
    Screen,
    /// Lighten, per-channel `max`.
    Lighten,
    /// Darken, per-channel `min`.
    Darken,
    /// Colour dodge.
    ColorDodge,
    /// Colour burn.
    ColorBurn,
    /// Hard light.
    HardLight,
    /// Soft light.
    SoftLight,
    /// Difference: per-channel absolute difference.
    Difference,
    /// Exclusion, a lower-contrast difference.
    Exclusion,
    /// Hue from source, saturation and luminosity from destination.
    Hue,
    /// Saturation from source, hue and luminosity from destination.
    Saturation,
    /// Color from source, luminosity from destination.
    Color,
    /// Luminosity from source, hue and saturation from destination.
    Luminosity,
}

impl BlendMode {
    /// Whether SVG can express this mode. SVG 1.1 has no blend modes at all;
    /// SVG 2 / CSS `mix-blend-mode` does, and every mode named here exists
    /// there, so an SVG carrying one of these is not invalid — it just needs a
    /// renderer that understands the newer spec. The non-separable four are the
    /// ones most SVG consumers skip, so they are the ones worth reporting.
    #[must_use]
    pub const fn is_widely_supported_in_svg(self) -> bool {
        !matches!(
            self,
            Self::Hue | Self::Saturation | Self::Color | Self::Luminosity
        )
    }
}

/// Which silhouette a shadow is cast from, and therefore where it falls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum ShadowKind {
    /// Blurred silhouette behind the box. The only kind implemented before
    /// v0.20, and the default so existing trees are unchanged.
    #[default]
    Drop,
    /// Shadow cast by the top edge only, inside the box. `inset 0 1px` in CSS.
    InsetTop,
    /// A hard-edged shadow offset inside the box, no blur. Useful for a
    /// pressed or engraved look, which a blurred drop shadow cannot express.
    InsetEdge,
}

/// How a mask derives coverage from its source image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MaskMode {
    /// The image's own alpha channel. The common case, and the only one that
    /// composites correctly over arbitrary backgrounds.
    #[default]
    Alpha,
    /// Rec. 709 luminance, so a black-and-white image masks by brightness. This
    /// is how a photographic image is used as a stencil.
    Luminance,
}

/// How a node's mask image is derived and applied.
///
/// A struct rather than two loose fields so `None` can mean "no mask" while a
/// set value with default members still means one, and so adding a mode later
/// cannot silently change the meaning of an existing tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct StyleMask {
    /// Which image supplies the mask.
    pub source: MaskSource,
    /// Whether coverage comes from alpha or luminance.
    #[serde(default)]
    pub mode: MaskMode,
}

/// Which image supplies a node's mask.
///
/// Kept separate from [`crate::Style`] because the mask image is opaque bytes
/// rather than a layout value, and because two nodes may share one image. It
/// lives on the node so a mask can be positioned by layout like any other image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum MaskSource {
    /// Use this node's own image bytes as the mask, fitted to its box.
    ///
    /// Convenient for the common "image with a stencil applied" case and costs
    /// no extra field: the image is already there.
    #[default]
    SelfImage,
}

/// Post-processing applied to a node and everything inside it.
///
/// # Order
///
/// CSS applies filter functions in source order. This is a fixed pipeline
/// instead, because [`crate::Style`] carries one struct rather than a list and
/// a reordered field would silently change output:
///
/// 1. **Colour matrix** -- `invert`, `grayscale`, `sepia`, `saturate`,
///    `contrast`, `brightness`. These all compose into a single 4x5 matrix, so
///    they cost one pass regardless of how many are set.
/// 2. **Opacity** -- last, so it scales the final coverage rather than the
///    inputs to the matrix.
///
/// Blur is deliberately absent. An earlier version implemented it as three box
/// passes and it was wrong in a way no test could pin down cheaply: repeated
/// passes progressively darkened the image rather than spreading it. Shipping a
/// filter that quietly eats a picture is worse than shipping no blur at all, so
/// the field was removed rather than left in place with a caveat. It remains open
/// work in the roadmap.
///
/// Every field is a factor with an identity value: `None` or `1.0` changes
/// nothing, so a default-constructed `Filter` is a no-op and is skipped
/// entirely rather than costing a pass.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Filter {
    /// `brightness(f)`: multiply every channel by `f`. `1.0` is identity.
    #[serde(default)]
    pub brightness: Option<f32>,
    /// `contrast(f)`: `f = 1.0` identity, `0.0` flat mid-grey.
    #[serde(default)]
    pub contrast: Option<f32>,
    /// `saturate(f)`: `1.0` identity, `0.0` fully desaturated.
    #[serde(default)]
    pub saturate: Option<f32>,
    /// `grayscale(f)`: blend toward luminance by `f`. `0.0` identity.
    #[serde(default)]
    pub grayscale: Option<f32>,
    /// `sepia(f)`: blend toward a sepia tone by `f`. `0.0` identity.
    #[serde(default)]
    pub sepia: Option<f32>,
    /// `invert(f)`: invert each channel by `f`. `0.0` identity.
    #[serde(default)]
    pub invert: Option<f32>,
    /// `opacity(f)`: scale final alpha by `f`. `1.0` identity.
    #[serde(default)]
    pub opacity: Option<f32>,
}

impl Filter {
    /// Does this filter do anything at all?
    ///
    /// Checked before allocating an offscreen buffer, because a no-op filter
    /// that went down the offscreen path would round pixels through a composite
    /// and change output for no reason.
    #[must_use]
    pub fn is_identity(&self) -> bool {
        self.brightness
            .is_none_or(|v| (v - 1.0).abs() < f32::EPSILON)
            && self.contrast.is_none_or(|v| (v - 1.0).abs() < f32::EPSILON)
            && self.saturate.is_none_or(|v| (v - 1.0).abs() < f32::EPSILON)
            && self.grayscale.is_none_or(|v| v.abs() < f32::EPSILON)
            && self.sepia.is_none_or(|v| v.abs() < f32::EPSILON)
            && self.invert.is_none_or(|v| v.abs() < f32::EPSILON)
            && self.opacity.is_none_or(|v| (v - 1.0).abs() < f32::EPSILON)
    }

    /// The combined colour matrix, folded into a single 4x5 affine transform.
    ///
    /// Row `r` reads `out_r = m[r][0]*r + m[r][1]*g + m[r][2]*b + m[r][4]`.
    /// Row 3 is alpha and is left alone; [`Self::opacity`] handles that, after
    /// the matrix, so a colour operation cannot disturb it.
    ///
    /// Returns `None` when no colour operation is set, which is the common case
    /// and lets the rasteriser skip the pass entirely.
    #[must_use]
    pub fn color_matrix(&self) -> Option<[[f32; 5]; 4]> {
        if self.invert.is_none()
            && self.grayscale.is_none()
            && self.sepia.is_none()
            && self.saturate.is_none()
            && self.contrast.is_none()
            && self.brightness.is_none()
        {
            return None;
        }

        // Identity for the three colour rows, and all-zero for the alpha row.
        // The alpha row is deliberately *not* `1 0 0 0 0`: alpha is applied
        // separately and after the matrix, so writing an identity into row 3
        // would leave `filter_tests::alpha_row_is_never_touched` failing and,
        // worse, would make every colour operation write to alpha.
        //
        // Each operation then folds in left to right in the order documented on
        // the type, so the result is one matrix whatever order the fields are
        // declared in.
        let mut m = [
            [1.0f32, 0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 0.0, 0.0],
        ];

        // invert(f): lerp(c, 1-c, f), i.e. c*(1-2f) + f. The offset is in the
        // same 0..=255 units as the channels.
        if let Some(f) = self.invert {
            fold(&mut m, [1.0 - 2.0 * f; 3], f * 255.0);
        }
        // grayscale(f): lerp(c, luma, f). This is *not* a per-channel scale: all
        // three output rows get the same luma weights, because every channel
        // becomes the same luminance. Folding it as a diagonal was the first
        // version's bug and turned "red becomes dark grey" into "red stays red".
        if let Some(f) = self.grayscale {
            fold_luma(&mut m, f);
        }
        // sepia(f): lerp(c, sepia(c), f).
        if let Some(f) = self.sepia {
            const S: [[f32; 3]; 3] = [
                [0.393, 0.769, 0.189],
                [0.349, 0.686, 0.168],
                [0.272, 0.534, 0.131],
            ];
            // `sepia(c)` mixes every input channel into every output channel, so
            // this is a full 3x3 rather than a diagonal: an identity part on the
            // diagonal plus the sepia tone everywhere.
            let mut w = [[0.0f32; 3]; 3];
            for (r, row) in w.iter_mut().enumerate() {
                for (c, cell) in row.iter_mut().enumerate() {
                    *cell = if r == c { 1.0 - f } else { 0.0 } + f * S[r][c];
                }
            }
            fold3(&mut m, w, 0.0);
        }
        // saturate(f) is grayscale with the parameter the other way round: CSS
        // defines f=1 as identity and f=0 as grey, so the colour term is scaled
        // by `f` and the luma term by `1-f`. Using grayscale's `1-f, f` here
        // made saturate(1) fully desaturate, which is the exact inverse of what
        // it is documented to do.
        if let Some(f) = self.saturate {
            fold_luma(&mut m, 1.0 - f);
        }
        // contrast(f): c*f + 0.5*(1-f), pivoting about mid grey. The offset is
        // scaled into channel units; leaving it in 0..=1 made `contrast(0)`
        // produce black instead of flat mid grey.
        if let Some(f) = self.contrast {
            fold(&mut m, [f; 3], 0.5 * (1.0 - f) * 255.0);
        }
        // brightness(f): c*f.
        if let Some(f) = self.brightness {
            fold(&mut m, [f; 3], 0.0);
        }
        Some(m)
    }
}

/// Rec. 709 luminance weights. These sum to 1, which is why `fold_luma` needs no
/// offset term.
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// Fold `lerp(c, luma(c), t)` into the affine `m`.
///
/// Expanded, output channel `r` is `c[r]*(1-t) + sum_k LUMA[k]*c[k]*t`, so row
/// `r` carries the luma weights *plus* `(1-t)` on its own diagonal:
///
/// ```text
/// row_r[k] = LUMA[k]*t + (1-t) * (r == k)
/// ```
///
/// Giving every row the same weights is wrong at every setting except full
/// desaturation: at `t = 0` it produces a matrix that sums all three channels
/// instead of the identity, which is how `saturate(1.0)` came out as "add the
/// channels together" rather than "change nothing".
fn fold_luma(m: &mut [[f32; 5]; 4], t: f32) {
    let mut w = [[0.0f32; 3]; 3];
    for (r, row) in w.iter_mut().enumerate() {
        for (k, cell) in row.iter_mut().enumerate() {
            *cell = LUMA[k] * t;
        }
        row[r] += 1.0 - t;
    }
    // The row weight sums are all 1, so the offset column carries through
    // unchanged and no correction is needed.
    fold3(m, w, 0.0);
}

/// Fold a per-channel operation `out = c*w + offset` into the affine `m`.
///
/// All offsets and weights are in the same 0..=255 units the rasteriser works
/// in, so `contrast(0)` lands on mid grey rather than on black.
///
/// Composing `out_r = sum_i (sum_j m[r][j] c_j + m[r][4]) w_i + offset` gives
/// `new_m[r][j] = sum_i m[r][i] w_i` and
/// `new_m[r][4] = m[r][4] * sum(w) + offset`. Getting that second term right is
/// the whole reason this is a function rather than three inline multiplications:
/// `invert` has weights summing to `3 - 6f`, not 1, so dropping the factor
/// misplaces every pixel that had a non-zero offset from an earlier operation.
fn fold(m: &mut [[f32; 5]; 4], w: [f32; 3], offset: f32) {
    fold3(
        m,
        [[w[0], 0.0, 0.0], [0.0, w[1], 0.0], [0.0, 0.0, w[2]]],
        offset,
    );
}

/// Fold a full 3x3 colour operation into `m`.
///
/// `w` describes the operation as `new_out_r = sum_i w[r][i] * out_i`, and
/// composing that with the existing affine gives
///
/// ```text
/// new_m[r][j] = sum_i w[r][i] * old_m[i][j]
/// new_m[r][4] = sum_i w[r][i] * old_m[i][4]  +  offset
/// ```
///
/// The first line is `w` times `old`, **not** `old` times `w`. The first
/// version had it the other way round, which is invisible whenever `old` is the
/// identity -- so every single-operation test passed, grayscale included, and
/// only `saturate(1.0)` exposed it. That is why the composition test below
/// exists and why it pairs two *luma* operations rather than two diagonal ones.
///
/// The weight sum is per row, not taken from row 0: for `invert` the weights are
/// all equal so it would not matter, but a matrix whose rows sum differently
/// would silently gain or lose its offset on the other channels.
fn fold3(m: &mut [[f32; 5]; 4], w: [[f32; 3]; 3], offset: f32) {
    let old = *m;
    for r in 0..3 {
        for j in 0..3 {
            m[r][j] = w[r][0] * old[0][j] + w[r][1] * old[1][j] + w[r][2] * old[2][j];
        }
        let sum = w[r][0] + w[r][1] + w[r][2];
        m[r][4] = old[r][4] * sum + offset;
    }
}

/// How image content fills its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ImgFit {
    /// Fill box, center-crop overflow (default).
    #[default]
    Cover,
    /// Fit inside box, letterboxed on the background.
    Contain,
    /// Stretch to the box exactly.
    Fill,
}

/// One gradient stop: position 0..=1 with a color.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorStop {
    /// Position along the gradient, 0..=1.
    pub pos: f32,
    /// Stop color.
    pub color: Color,
}

/// Background fill: solid color or gradient.
///
/// Accepts the shorthand `"#rrggbb"` in addition to `{"Solid": …}` in JSON,
/// since a solid fill is the overwhelmingly common case and forcing the
/// externally tagged form for it is pure ceremony. The tagged gradient forms
/// are unchanged, so this is backwards compatible with existing input.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum Background {
    /// Flat fill.
    Solid(Color),
    /// Linear gradient at CSS angle (0deg = to top, 180deg = to bottom).
    Linear {
        /// CSS angle in degrees.
        angle_deg: f32,
        /// Stops in order.
        stops: Vec<ColorStop>,
    },
    /// Radial gradient from a center point (fractions of the box).
    Radial {
        /// Center x as a fraction of width.
        cx: f32,
        /// Center y as a fraction of height.
        cy: f32,
        /// Radius in px (`<= 0` means cover-the-box default).
        radius: f32,
        /// Stops in order.
        stops: Vec<ColorStop>,
    },
}

impl Background {
    /// Solid fill from hex.
    #[must_use]
    pub fn solid(hex: &str) -> Self {
        Self::Solid(Color::from_hex(hex))
    }
}

/// Deserializes from either the shorthand form (`"#rrggbb"`, or any value
/// `Color` accepts) or the externally tagged form (`{"Solid": …}`,
/// `{"Linear": …}`, `{"Radial": …}`).
impl<'de> Deserialize<'de> for Background {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        #[allow(dead_code, non_snake_case)]
        enum Repr {
            /// Bare colour -> solid fill. `Color` itself accepts a hex string
            /// or an `{r,g,b,a}` map, so both shorthands work.
            Shorthand(Color),
            /// Single-key maps reproduce the externally tagged form.
            Solid {
                #[allow(dead_code)]
                Solid: Color,
            },
            Linear {
                #[allow(dead_code)]
                Linear: LinearDef,
            },
            Radial {
                #[allow(dead_code)]
                Radial: RadialDef,
            },
        }
        #[derive(Deserialize)]
        struct LinearDef {
            angle_deg: f32,
            stops: Vec<ColorStop>,
        }
        #[derive(Deserialize)]
        struct RadialDef {
            cx: f32,
            cy: f32,
            radius: f32,
            stops: Vec<ColorStop>,
        }

        Ok(match Repr::deserialize(d)? {
            Repr::Shorthand(c) | Repr::Solid { Solid: c } => Self::Solid(c),
            Repr::Linear { Linear: l } => Self::Linear {
                angle_deg: l.angle_deg,
                stops: l.stops,
            },
            Repr::Radial { Radial: r } => Self::Radial {
                cx: r.cx,
                cy: r.cy,
                radius: r.radius,
                stops: r.stops,
            },
        })
    }
}

/// Sample a background fill at box-relative point (`px`, `py`) in a
/// `w` x `h` box. Powers `background-clip: text` in raster paint.
#[must_use]
pub fn sample_background(bg: &Background, w: f32, h: f32, px: f32, py: f32) -> Color {
    match bg {
        Background::Solid(c) => *c,
        Background::Linear { angle_deg, stops } => {
            let ((x0, y0), (x1, y1)) = gradient_line(*angle_deg, 0.0, 0.0, w, h);
            let dx = x1 - x0;
            let dy = y1 - y0;
            let len2 = (dx * dx + dy * dy).max(1e-6);
            let t = (((px - x0) * dx + (py - y0) * dy) / len2).clamp(0.0, 1.0);
            sample_stops(stops, t)
        }
        Background::Radial {
            cx,
            cy,
            radius,
            stops,
        } => {
            let rad = if *radius > 0.0 {
                *radius
            } else {
                (w * w + h * h).sqrt() / 2.0
            };
            let d = ((px - cx * w).powi(2) + (py - cy * h).powi(2)).sqrt();
            sample_stops(stops, (d / rad.max(1e-6)).clamp(0.0, 1.0))
        }
    }
}

fn sample_stops(stops: &[ColorStop], t: f32) -> Color {
    if stops.is_empty() {
        return Color::rgb(0, 0, 0);
    }
    let mut prev = &stops[0];
    if t <= prev.pos {
        return prev.color;
    }
    for s in &stops[1..] {
        if t <= s.pos {
            let span = (s.pos - prev.pos).max(1e-6);
            let k = ((t - prev.pos) / span).clamp(0.0, 1.0);
            let mix = |a: u8, b: u8| (f32::from(a) * (1.0 - k) + f32::from(b) * k).round() as u8;
            return Color {
                r: mix(prev.color.r, s.color.r),
                g: mix(prev.color.g, s.color.g),
                b: mix(prev.color.b, s.color.b),
                a: mix(prev.color.a, s.color.a),
            };
        }
        prev = s;
    }
    stops[stops.len() - 1].color
}

/// Gradient line endpoints for a CSS angle inside a box.
///
/// Returns `((x0, y0), (x1, y1))` in the same coordinate space as the box.
#[must_use]
pub fn gradient_line(angle_deg: f32, x: f32, y: f32, w: f32, h: f32) -> ((f32, f32), (f32, f32)) {
    let rad = angle_deg.to_radians();
    let (dx, dy) = (rad.sin(), -rad.cos());
    let half = (w.abs() * dx.abs() + h.abs() * dy.abs()) / 2.0;
    let (cx, cy) = (x + w / 2.0, y + h / 2.0);
    (
        (cx - dx * half, cy - dy * half),
        (cx + dx * half, cy + dy * half),
    )
}

/// Layout-affecting style.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Style {
    /// Fixed width in px.
    #[serde(default)]
    pub width: Option<f32>,
    /// Fixed height in px.
    #[serde(default)]
    pub height: Option<f32>,
    /// Word-wrap text at this width in px (text only).
    #[serde(default)]
    pub max_width: Option<f32>,
    /// Width/height ratio used when one side is unspecified (images).
    #[serde(default)]
    pub aspect: Option<f32>,
    /// Layout algorithm.
    #[serde(default)]
    pub display: Display,
    /// Grid column count (`display == Grid`).
    #[serde(default)]
    pub grid_cols: Option<u16>,
    /// Flex direction (`display == Flex`).
    #[serde(default)]
    pub dir: FlexDir,
    /// Main-axis alignment.
    #[serde(default)]
    pub justify: Justify,
    /// Cross-axis alignment.
    #[serde(default)]
    pub align: Align,
    /// Gap between children in px.
    #[serde(default)]
    pub gap: f32,
    /// Uniform padding in px.
    #[serde(default)]
    pub padding: f32,
    /// Uniform margin in px (transparent).
    #[serde(default)]
    pub margin: f32,
    /// Border width in px. Layout reserves the space.
    #[serde(default)]
    pub border: f32,
    /// Border color.
    #[serde(default)]
    pub border_color: Option<Color>,
    /// Take out of flow, positioned vs the nearest box.
    #[serde(default)]
    pub absolute: bool,
    /// Absolute left offset in px.
    #[serde(default)]
    pub left: Option<f32>,
    /// Absolute top offset in px.
    #[serde(default)]
    pub top: Option<f32>,
    /// Background fill.
    #[serde(default)]
    pub background: Option<Background>,
    /// Text color.
    #[serde(default)]
    pub color: Option<Color>,
    /// Font size in px.
    #[serde(default)]
    pub font_size: Option<f32>,

    /// Font to shape and paint this node with, as a [`FontId`] returned by
    /// [`register_font`]. `None` uses the embedded font.
    ///
    /// Serialized as a plain number so a tree round-trips through the Node and
    /// WASM bindings unchanged: register the font once, then put the id here.
    #[serde(default)]
    pub font: Option<FontId>,
    /// Border radius in px.
    #[serde(default)]
    pub radius: f32,
    /// Flex grow factor.
    #[serde(default)]
    pub grow: f32,
    /// Box shadow (painted beneath border/background).
    #[serde(default)]
    pub shadow: Option<Shadow>,
    /// Blend mode for this node's content against what is beneath it.
    /// `None` means [`BlendMode::Normal`], so a tree that names none renders
    /// exactly as it did before blend modes existed.
    #[serde(default)]
    pub blend: Option<BlendMode>,
    /// PDF bookmark level (`None` = not in outline).
    #[serde(default)]
    pub bookmark: Option<u8>,
    /// Keep unsplit across flow pages.
    #[serde(default)]
    pub keep_together: bool,
    /// Repeat atop every continuation page when flowing.
    #[serde(default)]
    pub repeat_header: bool,
    /// Hyperlink URL for PDF link annotations.
    #[serde(default)]
    pub link: Option<String>,
    /// Paint text with the background fill (`background-clip: text`).
    #[serde(default)]
    pub clip_text: bool,

    /// Post-processing applied to this node and everything inside it.
    ///
    /// `None`, and any filter whose fields are all at identity, render exactly
    /// as if no filter were set -- the rasteriser checks for that and skips the
    /// offscreen pass rather than rounding pixels through a composite.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<Filter>,

    /// Mask this node's contents using one of its images.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<StyleMask>,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            width: None,
            height: None,
            max_width: None,
            aspect: None,
            display: Display::Flex,
            grid_cols: None,
            dir: FlexDir::Row,
            justify: Justify::Start,
            align: Align::Start,
            gap: 0.0,
            padding: 0.0,
            margin: 0.0,
            border: 0.0,
            border_color: None,
            absolute: false,
            left: None,
            top: None,
            background: None,
            color: None,
            blend: None,
            font_size: None,
            font: None,
            radius: 0.0,
            grow: 0.0,
            bookmark: None,
            keep_together: false,
            repeat_header: false,
            shadow: None,
            link: None,
            clip_text: false,
            filter: None,
            mask: None,
        }
    }
}

impl Style {
    /// Empty style.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Row container.
    #[must_use]
    pub fn row() -> Self {
        Self {
            dir: FlexDir::Row,
            ..Self::default()
        }
    }

    /// Column container.
    #[must_use]
    pub fn column() -> Self {
        Self {
            dir: FlexDir::Column,
            ..Self::default()
        }
    }

    /// Centered row.
    #[must_use]
    pub fn centered() -> Self {
        Self {
            dir: FlexDir::Row,
            justify: Justify::Center,
            align: Align::Center,
            ..Self::default()
        }
    }

    /// Text style helper.
    #[must_use]
    pub fn text(font_size: f32, color_hex: &str) -> Self {
        Self {
            font_size: Some(font_size),
            color: Some(Color::from_hex(color_hex)),
            ..Self::default()
        }
    }

    /// Set fixed size.
    #[must_use]
    pub fn with_size(mut self, w: f32, h: f32) -> Self {
        self.width = Some(w);
        self.height = Some(h);
        self
    }

    /// Set background from hex.
    #[must_use]
    pub fn with_background(mut self, hex: &str) -> Self {
        self.background = Some(Background::Solid(Color::from_hex(hex)));
        self
    }

    /// Linear-gradient background. `stops` are `(position 0..=1, hex)` pairs.
    #[must_use]
    pub fn with_linear_gradient(mut self, angle_deg: f32, stops: &[(f32, &str)]) -> Self {
        self.background = Some(Background::Linear {
            angle_deg,
            stops: stops
                .iter()
                .map(|(pos, hex)| ColorStop {
                    pos: *pos,
                    color: Color::from_hex(hex),
                })
                .collect(),
        });
        self
    }

    /// Radial-gradient background centered at box fractions (`0.5, 0.5` center).
    #[must_use]
    pub fn with_radial_gradient(
        mut self,
        cx: f32,
        cy: f32,
        radius: f32,
        stops: &[(f32, &str)],
    ) -> Self {
        self.background = Some(Background::Radial {
            cx,
            cy,
            radius,
            stops: stops
                .iter()
                .map(|(pos, hex)| ColorStop {
                    pos: *pos,
                    color: Color::from_hex(hex),
                })
                .collect(),
        });
        self
    }

    /// Set padding.
    #[must_use]
    pub fn with_padding(mut self, px: f32) -> Self {
        self.padding = px;
        self
    }

    /// Set gap.
    #[must_use]
    pub fn with_gap(mut self, px: f32) -> Self {
        self.gap = px;
        self
    }

    /// Set radius.
    #[must_use]
    pub fn with_radius(mut self, px: f32) -> Self {
        self.radius = px;
        self
    }

    /// Set justify.
    #[must_use]
    pub fn with_justify(mut self, j: Justify) -> Self {
        self.justify = j;
        self
    }

    /// Set align.
    #[must_use]
    pub fn with_align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }

    /// Equal-column grid container.
    #[must_use]
    pub fn grid(cols: u16) -> Self {
        Self {
            display: Display::Grid,
            grid_cols: Some(cols.max(1)),
            ..Self::default()
        }
    }

    /// Set uniform margin.
    #[must_use]
    pub fn with_margin(mut self, px: f32) -> Self {
        self.margin = px;
        self
    }

    /// Set border width + color.
    #[must_use]
    pub fn with_border(mut self, px: f32, hex: &str) -> Self {
        self.border = px;
        self.border_color = Some(Color::from_hex(hex));
        self
    }

    /// Drop shadow: offset + blur + spread with a color.
    #[must_use]
    pub fn with_shadow(mut self, dx: f32, dy: f32, blur: f32, spread: f32, hex: &str) -> Self {
        self.shadow = Some(Shadow {
            kind: ShadowKind::Drop,
            dx,
            dy,
            blur: blur.max(0.0),
            spread,
            color: Color::from_hex(hex),
        });
        self
    }

    /// Box shadow cast by a specific edge. The `Drop` case is
    /// [`with_shadow`](Self::with_shadow); the inset kinds land inside the box,
    /// which is what a pressed or engraved look needs and a blurred drop shadow
    /// cannot express.
    #[must_use]
    pub fn with_shadow_kind(
        mut self,
        kind: ShadowKind,
        dx: f32,
        dy: f32,
        blur: f32,
        spread: f32,
        hex: &str,
    ) -> Self {
        self.shadow = Some(Shadow {
            kind,
            dx,
            dy,
            blur: blur.max(0.0),
            spread,
            color: Color::from_hex(hex),
        });
        self
    }

    /// Combine this node's content with what is beneath it. The default,
    /// [`BlendMode::Normal`], is plain alpha compositing and is what every
    /// render did before this existed.
    #[must_use]
    pub fn with_blend_mode(mut self, mode: BlendMode) -> Self {
        self.blend = Some(mode);
        self
    }

    /// Take out of flow at `(left, top)`.
    #[must_use]
    pub fn absolute_at(mut self, left: f32, top: f32) -> Self {
        self.absolute = true;
        self.left = Some(left);
        self.top = Some(top);
        self
    }

    /// Word-wrap text at this width.
    #[must_use]
    pub fn with_max_width(mut self, px: f32) -> Self {
        self.max_width = Some(px);
        self
    }

    /// List this text in the PDF outline (bookmarks) at `level` (1 = top).
    #[must_use]
    pub fn with_bookmark(mut self, level: u8) -> Self {
        self.bookmark = Some(level.max(1));
        self
    }

    /// Keep this box unsplit when flowing across pages (moves whole).
    /// Post-process this node and everything inside it.
    ///
    /// `#[must_use]` like every other setter here, so a forgotten `let _ =` does
    /// not silently drop the filter and leave a card looking unfiltered with
    /// nothing in the output to explain why.
    #[must_use]
    pub fn with_filter(mut self, filter: Filter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Mask this node's contents using one of its images.
    #[must_use]
    pub fn with_mask(mut self, mask: StyleMask) -> Self {
        self.mask = Some(mask);
        self
    }

    /// Keep this box unsplit when flowing across pages (moves whole).
    #[must_use]
    pub fn keep_together(mut self) -> Self {
        self.keep_together = true;
        self
    }

    /// Repeat this child atop every continuation page when flowing.
    #[must_use]
    pub fn repeat_header(mut self) -> Self {
        self.repeat_header = true;
        self
    }

    /// Hyperlink for PDF link annotations (raster/SVG ignore it).
    #[must_use]
    pub fn with_link(mut self, url: &str) -> Self {
        self.link = Some(url.to_owned());
        self
    }

    /// Set the font to shape and paint with, by id from [`register_font`].
    #[must_use]
    pub fn with_font(mut self, font: FontId) -> Self {
        self.font = Some(font);
        self
    }

    /// Paint text glyphs with the background fill instead of `color`
    /// (`background-clip: text`; needs a background to sample from).
    #[must_use]
    pub fn clip_text(mut self) -> Self {
        self.clip_text = true;
        self
    }

    fn to_taffy(&self, text: Option<&str>, media: Option<(u32, u32)>) -> TaffyStyle {
        use taffy::style::{Display as TaffyDisplay, LengthPercentageAuto, Position};
        use taffy::style_helpers::TaffyAuto as _;
        let dim = |v: Option<f32>| match v {
            Some(px) => Dimension::length(px),
            None => Dimension::auto(),
        };
        let auto_or = |v: Option<f32>| match v {
            Some(px) => LengthPercentageAuto::length(px),
            None => LengthPercentageAuto::AUTO,
        };
        // Intrinsic sizes so layout and paint agree: shaped text, decoded images.
        let (mut w, mut h) = (dim(self.width), dim(self.height));
        if let Some(t) = text {
            let fs = self.font_size.unwrap_or(16.0);
            let font = self.font.unwrap_or(BUILTIN_FONT);
            let laid = match self.max_width {
                Some(mw) => wrap_text(t, fs, mw, font),
                None => t.to_owned(),
            };
            let (mw, mh) = measure_text(&laid, fs, font);
            if self.width.is_none() {
                w = Dimension::length(mw.max(1.0));
            }
            if self.height.is_none() {
                h = Dimension::length(mh.max(1.0));
            }
        }
        if let Some((iw, ih)) = media {
            let ratio = self.aspect.unwrap_or(iw as f32 / ih.max(1) as f32);
            match (self.width, self.height) {
                (None, None) => {
                    w = Dimension::length(iw as f32);
                    h = Dimension::length(ih as f32);
                }
                (None, Some(hh)) => {
                    w = Dimension::length(hh * ratio);
                }
                (Some(ww), None) => {
                    h = Dimension::length(ww / ratio);
                }
                (Some(_), Some(_)) => {}
            }
        }
        let cols = match self.display {
            Display::Grid => self.grid_cols.unwrap_or(1).max(1),
            _ => 0,
        };
        // CSS grid defaults items to stretch; flex keeps our explicit default.
        let grid = matches!(self.display, Display::Grid);
        TaffyStyle {
            size: Size {
                width: w,
                height: h,
            },
            display: match self.display {
                Display::Flex => TaffyDisplay::Flex,
                Display::Grid => TaffyDisplay::Grid,
                Display::Block => TaffyDisplay::Block,
            },
            grid_template_columns: (0..cols).map(|_| taffy::prelude::fr(1.0_f32)).collect(),
            flex_direction: match self.dir {
                FlexDir::Row => FlexDirection::Row,
                FlexDir::Column => FlexDirection::Column,
            },
            justify_content: Some(match self.justify {
                Justify::Start => JustifyContent::Start,
                Justify::Center => JustifyContent::Center,
                Justify::End => JustifyContent::End,
                Justify::SpaceBetween => JustifyContent::SpaceBetween,
                Justify::SpaceAround => JustifyContent::SpaceAround,
            }),
            align_items: Some(match self.align {
                Align::Start if grid => AlignItems::Stretch,
                Align::Start => AlignItems::Start,
                Align::Center => AlignItems::Center,
                Align::End => AlignItems::End,
                Align::Stretch => AlignItems::Stretch,
            }),
            justify_items: if grid {
                Some(AlignItems::Stretch)
            } else {
                None
            },
            gap: Size {
                width: LengthPercentage::length(self.gap),
                height: LengthPercentage::length(self.gap),
            },
            padding: taffy::geometry::Rect {
                left: LengthPercentage::length(self.padding),
                right: LengthPercentage::length(self.padding),
                top: LengthPercentage::length(self.padding),
                bottom: LengthPercentage::length(self.padding),
            },
            margin: taffy::geometry::Rect {
                left: LengthPercentageAuto::length(self.margin),
                right: LengthPercentageAuto::length(self.margin),
                top: LengthPercentageAuto::length(self.margin),
                bottom: LengthPercentageAuto::length(self.margin),
            },
            border: taffy::geometry::Rect {
                left: LengthPercentage::length(self.border),
                right: LengthPercentage::length(self.border),
                top: LengthPercentage::length(self.border),
                bottom: LengthPercentage::length(self.border),
            },
            position: if self.absolute {
                Position::Absolute
            } else {
                Position::Relative
            },
            inset: taffy::geometry::Rect {
                left: auto_or(self.left),
                right: LengthPercentageAuto::AUTO,
                top: auto_or(self.top),
                bottom: LengthPercentageAuto::AUTO,
            },
            aspect_ratio: self.aspect,
            flex_grow: self.grow,
            ..Default::default()
        }
    }
}

/// Render node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Node {
    /// Container with children.
    Container {
        /// Box style.
        style: Style,
        /// Children.
        children: Vec<Node>,
    },
    /// Single text run.
    Text {
        /// UTF-8 content.
        text: String,
        /// Text + box style.
        style: Style,
    },
    /// Decoded-at-paint image (PNG/JPEG bytes).
    Image {
        /// Encoded image bytes.
        ///
        /// Empty when the node was declared with a `src` name instead; see
        /// [`crate::assets::Assets`] for resolving one into bytes.
        bytes: Vec<u8>,
        /// Asset name to resolve bytes from, or `None` when the bytes are inline.
        ///
        /// Kept alongside `bytes` rather than instead of it, so a tree that is
        /// fully resolved still renders on its own and needs no second pass. A
        /// resolved tree simply has both, and `bytes` wins.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        src: Option<String>,
        /// Fill behavior.
        fit: ImgFit,
        /// Box style.
        style: Style,
    },
}

impl Node {
    /// Container constructor.
    #[must_use]
    pub fn container(style: Style, children: Vec<Node>) -> Self {
        Self::Container { style, children }
    }

    /// Full-bleed centered banner helper (the OG-image 90% case).
    #[must_use]
    pub fn banner(
        width: f32,
        height: f32,
        bg_hex: &str,
        text: &str,
        font_size: f32,
        fg_hex: &str,
    ) -> Self {
        Self::container(
            Style::centered()
                .with_size(width, height)
                .with_background(bg_hex),
            vec![Self::Text {
                text: text.to_owned(),
                style: Style::text(font_size, fg_hex),
            }],
        )
    }

    /// Text constructor.
    #[must_use]
    pub fn text(content: &str, style: Style) -> Self {
        Self::Text {
            text: content.to_owned(),
            style,
        }
    }

    /// An image node whose bytes come from an [`crate::assets::Assets`] entry.
    ///
    /// Takes a name rather than bytes so a tree can be declared before anything
    /// is fetched. The name is only a reference: rendering an unresolved tree
    /// reports a missing asset rather than painting a blank box.
    #[must_use]
    pub fn image_ref(src: &str, style: Style) -> Self {
        Self::Image {
            bytes: Vec::new(),
            src: Some(src.to_owned()),
            fit: ImgFit::Cover,
            style,
        }
    }

    /// Image constructor (`fit` defaults to cover).
    #[must_use]
    pub fn image(bytes: Vec<u8>, style: Style) -> Self {
        Self::Image {
            bytes,
            src: None,
            fit: ImgFit::Cover,
            style,
        }
    }
}

/// Positioned node after layout.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// X in px relative to viewport.
    pub x: f32,
    /// Y in px relative to viewport.
    pub y: f32,
    /// Width in px.
    pub w: f32,
    /// Height in px.
    pub h: f32,
    /// Style at this box.
    pub style: Style,
    /// Wrapped text if leaf.
    pub text: Option<String>,
    /// Hyperlink URL if leaf.
    pub link: Option<String>,
    /// Image payload if leaf.
    pub media: Option<Media>,
    /// Children in order.
    pub children: Vec<Placed>,
}

/// Paint payload carried from layout to raster.
#[derive(Debug, Clone, PartialEq)]
pub enum Media {
    /// Image bytes + fill behavior.
    Image {
        /// Encoded bytes.
        bytes: Vec<u8>,
        /// Fill behavior.
        fit: ImgFit,
    },
}

/// Intrinsic image dimensions, read from the header only.
///
/// This used to fully decode the image. The header already carries the
/// dimensions -- for a PNG they are two u32s at offset 16 -- so the decode was
/// decompressing every pixel of every image purely to learn how big it was.
///
/// The cost was invisible because the card benchmarks have no images in them.
/// A workload benchmark with twelve images found it immediately: **layout went
/// from 4 us on a card to 3.97 ms**, a thousandfold, for images that were
/// never drawn any differently. Layout was doing the painting's work and the
/// paint stage was doing it again.
///
/// This reads the header and stops; [peek_image_dimensions] says how and what
/// it costs. A malformed image still errors rather than silently laying out
/// at some fallback size.
pub fn image_dimensions(bytes: &[u8]) -> Result<(u32, u32), Error> {
    peek_image_dimensions(bytes).map_err(|e| Error::Asset(format!("image header: {e}")))
}

/// A failure to read a container header. Private: `image_dimensions` turns
/// them into the public error type, and callers only ever see that.
enum HeaderError {
    Truncated { need: usize, have: usize },
    Unrecognised,
    Malformed(&'static str),
}

impl std::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeaderError::Truncated { need, have } => {
                write!(f, "needs {need} bytes for the header, have {have}")
            }
            HeaderError::Unrecognised => {
                write!(f, "not a supported container (png, jpeg, gif, webp)")
            }
            HeaderError::Malformed(why) => write!(f, "{why}"),
        }
    }
}

fn u16be(b: &[u8]) -> u16 {
    u16::from_be_bytes([b[0], b[1]])
}

fn u32be(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

fn u16le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

fn u24le(b: &[u8]) -> u32 {
    u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16
}

/// Intrinsic dimensions straight from the container header.
///
/// Every format this crate decodes (png, jpeg, gif, webp) records its pixel
/// size in a fixed place in the first bytes of the file; all four are read
/// here. Two properties matter and the tests pin both:
///
/// 1. **Same answer as the decoder.** For every valid file, what this returns
///    must equal what `image::load_from_memory` reports for the same file.
///    The differential test covers all four formats at several sizes,
///    including 1x1 and non-round numbers. A header parser is the kind of
///    code that drifts silently; agreement with the real decoder is what
///    keeps it pinned to reality.
/// 2. **Failure only, never a lie.** Insufficient or mutated input returns an
///    error. The old path gated layout on a full successful decode, so an
///    image whose header parse succeeds but whose body is corrupt would have
///    failed later in paint anyway; the header path moves that failure from
///    paint to layout, which is where a size question belongs. The fuzz
///    corpus asserts every mutated or truncated input yields `Ok` or `Err`
///    and never a panic.
///
/// This path deliberately does not go through `image::ImageReader`. That
/// reader is generic over `R: Read`, so using it here monomorphises a second
/// copy of every format decoder next to the one the raster backend already
/// carries via `load_from_memory` -- +186 KB of wasm for zero new capability,
/// which is what paid for this function to exist.
fn peek_image_dimensions(b: &[u8]) -> Result<(u32, u32), HeaderError> {
    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    if b.len() >= 8 && b[..8] == PNG {
        if b.len() < 24 {
            return Err(HeaderError::Truncated {
                need: 24,
                have: b.len(),
            });
        }
        // IHDR must be the first chunk: four bytes of length, then the name.
        if &b[12..16] != b"IHDR" {
            return Err(HeaderError::Malformed("first chunk is not IHDR"));
        }
        let (w, h) = (u32be(&b[16..20]), u32be(&b[20..24]));
        if w == 0 || h == 0 {
            return Err(HeaderError::Malformed("PNG IHDR declares a zero dimension"));
        }
        return Ok((w, h));
    }

    // GIF: 6-byte signature, then the logical screen descriptor -- two
    // little-endian u16s, wide then high.
    if b.len() >= 10 && (&b[..4] == b"GIF8") && (b[4] == b'7' || b[4] == b'9') && b[5] == b'a' {
        let (w, h) = (u32::from(u16le(&b[6..8])), u32::from(u16le(&b[8..10])));
        if w == 0 || h == 0 {
            return Err(HeaderError::Malformed(
                "GIF logical screen declares a zero dimension",
            ));
        }
        return Ok((w, h));
    }

    // WebP: RIFF container with a WEBP type, then a chunk describing the
    // stream. Shapes differ by chunk: VP8X (extended, canvas at 24),
    // VP8 (lossy keyframe, sync code then 14-bit sizes), VP8L (lossless,
    // packed 14-bit sizes after the 0x2F signature).
    if b.len() >= 12 && &b[..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        if b.len() < 16 {
            return Err(HeaderError::Truncated {
                need: 16,
                have: b.len(),
            });
        }
        let chunk = &b[12..16];
        match (chunk, b.len()) {
            (b"VP8X", _) => {
                if b.len() < 30 {
                    return Err(HeaderError::Truncated {
                        need: 30,
                        have: b.len(),
                    });
                }
                // Canvas size is stored as value-one, 24 bits each.
                let (w, h) = (u24le(&b[24..27]) + 1, u24le(&b[27..30]) + 1);
                return Ok((w, h));
            }
            (b"VP8 ", n) => {
                if n < 30 {
                    return Err(HeaderError::Truncated { need: 30, have: n });
                }
                if b[23..26] != [0x9D, 0x01, 0x2A] {
                    return Err(HeaderError::Malformed("VP8 chunk missing the start code"));
                }
                // Width and height are the low 14 bits of each u16.
                let (w, h) = (
                    u32::from(u16le(&b[26..28]) & 0x3FFF),
                    u32::from(u16le(&b[28..30]) & 0x3FFF),
                );
                if w == 0 || h == 0 {
                    return Err(HeaderError::Malformed(
                        "VP8 keyframe declares a zero dimension",
                    ));
                }
                return Ok((w, h));
            }
            (b"VP8L", n) => {
                if n < 25 {
                    return Err(HeaderError::Truncated { need: 25, have: n });
                }
                if b[20] != 0x2F {
                    return Err(HeaderError::Malformed(
                        "VP8L chunk missing its signature byte",
                    ));
                }
                let bits = u32::from_le_bytes([b[21], b[22], b[23], b[24]]);
                let (w, h) = ((bits & 0x3FFF) + 1, ((bits >> 14) & 0x3FFF) + 1);
                return Ok((w, h));
            }
            _ => return Err(HeaderError::Malformed("unrecognised WebP chunk")),
        }
    }

    // JPEG: scan the segment stream for a frame header. Every segment is a
    // marker plus a big-endian length; the frame markers are the SOFn family,
    // which carry height then width just after the sample precision byte.
    if b.len() >= 4 && b[0] == 0xFF && b[1] == 0xD8 {
        let mut i = 2;
        while i + 4 <= b.len() {
            // All the markers we skip carry a two-byte length after the
            // marker itself; standalone markers (RST*, TEM) are legal JPEG
            // but never precede a frame header, so seeing one means the
            // stream is not what we can parse.
            if b[i] != 0xFF || b[i + 1] == 0x00 {
                break;
            }
            let marker = b[i + 1];
            if marker == 0xD8 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
                i += 2;
                continue;
            }
            // 0xFFD9 is end of image; a frame header cannot follow it.
            if marker == 0xD9 || marker == 0xDA {
                break;
            }
            let seg_len = usize::from(u16be(&b[i + 2..i + 4]));
            if seg_len < 2 {
                break;
            }
            let has_components =
                (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
            if has_components {
                // Frame header: marker(2) len(2) precision(1) h(2) w(2) --
                // height comes first in the file, and the tuple in this
                // crate is (w, h). The differential test caught the swap on
                // the first run of this code; a hand-rolled parser that is
                // not tested against the real decoder will not catch this.
                if i + 9 > b.len() {
                    return Err(HeaderError::Truncated {
                        need: i + 9,
                        have: b.len(),
                    });
                }
                let (h, w) = (
                    u32::from(u16be(&b[i + 5..i + 7])),
                    u32::from(u16be(&b[i + 7..i + 9])),
                );
                if h == 0 || w == 0 {
                    return Err(HeaderError::Malformed(
                        "JPEG frame header declares a zero dimension",
                    ));
                }
                return Ok((w, h));
            }
            i += 2 + seg_len;
        }
        return Err(HeaderError::Malformed(
            "no frame header in the segment stream",
        ));
    }

    Err(HeaderError::Unrecognised)
}

/// Compute flex layout for `tree` inside `viewport_w` x `viewport_h`.
pub fn compute_layout(tree: &Node, viewport_w: f32, viewport_h: f32) -> Result<Placed, Error> {
    let mut taffy: TaffyTree<()> = TaffyTree::new();
    let root_id = build_taffy(tree, &mut taffy, BUILTIN_FONT)?;
    taffy.compute_layout(
        root_id,
        Size {
            width: AvailableSpace::Definite(viewport_w),
            height: AvailableSpace::Definite(viewport_h),
        },
    )?;
    read_back(tree, &taffy, root_id, BUILTIN_FONT)
}

fn build_taffy(
    node: &Node,
    taffy: &mut TaffyTree<()>,
    inherited_font: FontId,
) -> Result<taffy::NodeId, Error> {
    match node {
        Node::Container { style, children } => {
            let font = style.font.unwrap_or(inherited_font);
            let ids = children
                .iter()
                .map(|c| build_taffy(c, taffy, font))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(
                taffy
                    .new_with_children(style.clone().with_font(font).to_taffy(None, None), &ids)?,
            )
        }
        Node::Text { text, style } => {
            let style = style
                .clone()
                .with_font(style.font.unwrap_or(inherited_font));
            Ok(taffy.new_leaf(style.to_taffy(Some(text), None))?)
        }
        Node::Image { bytes, style, .. } => {
            let dims = image_dimensions(bytes).ok();
            let style = style
                .clone()
                .with_font(style.font.unwrap_or(inherited_font));
            Ok(taffy.new_leaf(style.to_taffy(None, dims))?)
        }
    }
}

fn read_back(
    node: &Node,
    taffy: &TaffyTree<()>,
    id: taffy::NodeId,
    inherited_font: FontId,
) -> Result<Placed, Error> {
    // Layout failures propagate as `Error` rather than panicking. This runs on
    // untrusted input from the Node/WASM bindings, and unwinding across the FFI
    // boundary is undefined behaviour.
    let layout = taffy.layout(id)?;
    let (style, text, link, media, kids) = match node {
        Node::Container { style, children } => {
            let child_ids = taffy.children(id)?;
            let font = style.font.unwrap_or(inherited_font);
            let placed = children
                .iter()
                .zip(child_ids)
                .map(|(c, cid)| read_back(c, taffy, cid, font))
                .collect::<Result<Vec<_>, _>>()?;
            (
                style.clone().with_font(font),
                None,
                style.link.clone(),
                None,
                placed,
            )
        }
        Node::Text { text, style } => {
            // Store the wrapped text so paint wraps identically to layout.
            // `BUILTIN_FONT` rather than `inherited_font`: `build_taffy` already
            // wrote the resolved font onto this node's style, so an unset value
            // here means the caller genuinely did not set one.
            let font = style.font.unwrap_or(inherited_font);
            let laid = match style.max_width {
                Some(mw) => wrap_text(text, style.font_size.unwrap_or(16.0), mw, font),
                None => text.clone(),
            };
            (
                style.clone().with_font(font),
                Some(laid),
                style.link.clone(),
                None,
                Vec::new(),
            )
        }
        Node::Image {
            bytes, fit, style, ..
        } => (
            style.clone(),
            None,
            style.link.clone(),
            Some(Media::Image {
                bytes: bytes.clone(),
                fit: *fit,
            }),
            Vec::new(),
        ),
    };
    Ok(Placed {
        x: layout.location.x,
        y: layout.location.y,
        w: layout.size.width,
        h: layout.size.height,
        style,
        text,
        link,
        media,
        children: kids,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_banner_text() {
        let tree = Node::banner(1200.0, 630.0, "#0b1020", "Hi", 72.0, "#ffffff");
        let placed = compute_layout(&tree, 1200.0, 630.0).unwrap();
        assert_eq!((placed.w, placed.h), (1200.0, 630.0));
        assert_eq!(placed.children.len(), 1);
        let t = &placed.children[0];
        assert!(t.w > 0.0 && t.h > 0.0);
        // Centered: text box roughly in middle band.
        assert!((t.x + t.w / 2.0 - 600.0).abs() < 120.0);
        assert!((t.y + t.h / 2.0 - 315.0).abs() < 120.0);
    }

    #[test]
    fn hex_parses() {
        assert_eq!(Color::from_hex("#fff"), Color::rgb(255, 255, 255));
        assert_eq!(
            Color::from_hex("#0b1020"),
            Color {
                r: 11,
                g: 16,
                b: 32,
                a: 255
            }
        );
    }

    #[test]
    fn color_deserializes_from_string_or_map() {
        // The JSON bindings accept both, because forcing every colour to be a
        // four-key object made the most common field the most tedious one.
        let from_string: Color = serde_json::from_str("\"#0b1020\"").expect("hex string");
        assert_eq!(
            from_string,
            Color {
                r: 11,
                g: 16,
                b: 32,
                a: 255
            }
        );

        let from_short: Color = serde_json::from_str("\"#fff\"").expect("short hex");
        assert_eq!(from_short, Color::rgb(255, 255, 255));

        let with_alpha: Color = serde_json::from_str("\"#0b102080\"").expect("hex with alpha");
        assert_eq!(with_alpha.a, 128);

        let from_map: Color =
            serde_json::from_str(r#"{"r":11,"g":16,"b":32,"a":255}"#).expect("rgba map");
        assert_eq!(
            from_map,
            Color {
                r: 11,
                g: 16,
                b: 32,
                a: 255
            }
        );

        // Alpha is optional in the map form and defaults to opaque.
        let map_no_alpha: Color =
            serde_json::from_str(r#"{"r":1,"g":2,"b":3}"#).expect("rgba without alpha");
        assert_eq!(
            map_no_alpha,
            Color {
                r: 1,
                g: 2,
                b: 3,
                a: 255
            }
        );

        // Serialization is unchanged: still the struct form.
        let json = serde_json::to_string(&from_string).expect("serialize");
        assert!(json.contains("\"r\":11"), "unexpected shape: {json}");
    }

    #[test]
    fn background_accepts_shorthand_string() {
        let shorthand: Background = serde_json::from_str("\"#0b1020\"").expect("shorthand");
        assert_eq!(
            shorthand,
            Background::Solid(Color {
                r: 11,
                g: 16,
                b: 32,
                a: 255
            })
        );

        let tagged: Background =
            serde_json::from_str(r#"{"Solid":{"r":11,"g":16,"b":32,"a":255}}"#).expect("tagged");
        assert_eq!(tagged, shorthand, "both forms must agree");

        // Gradients keep their externally tagged form. Note the `r##` form:
        // a plain `r#` string would terminate at the `"#` in `"#000000"`.
        let gradient: Background = serde_json::from_str(
            r##"{"Linear":{"angle_deg":90,"stops":[{"pos":0.0,"color":"#000000"},{"pos":1.0,"color":"#ffffff"}]}}"##,
        )
        .expect("gradient");
        assert!(matches!(gradient, Background::Linear { .. }));
    }

    #[test]
    fn sample_background_midpoint() {
        let bg = Background::Linear {
            angle_deg: 90.0,
            stops: vec![
                ColorStop {
                    pos: 0.0,
                    color: Color::rgb(0, 0, 0),
                },
                ColorStop {
                    pos: 1.0,
                    color: Color::rgb(255, 255, 255),
                },
            ],
        };
        let mid = sample_background(&bg, 100.0, 50.0, 50.0, 25.0);
        assert!((mid.r as i16 - 128).abs() <= 2, "{mid:?}");
        assert_eq!(
            sample_background(&bg, 100.0, 50.0, 0.0, 25.0),
            Color::rgb(0, 0, 0)
        );
        assert_eq!(
            sample_background(&bg, 100.0, 50.0, 100.0, 25.0),
            Color::rgb(255, 255, 255)
        );
    }

    #[test]
    fn gradient_line_angles() {
        // 180deg (CSS `to bottom`): top-center -> bottom-center.
        let ((x0, y0), (x1, y1)) = gradient_line(180.0, 0.0, 0.0, 100.0, 200.0);
        assert!(
            (x0 - 50.0).abs() < 0.01 && (y0 - 0.0).abs() < 0.01,
            "{x0},{y0}"
        );
        assert!(
            (x1 - 50.0).abs() < 0.01 && (y1 - 200.0).abs() < 0.01,
            "{x1},{y1}"
        );
        // 90deg (`to right`): left-center -> right-center.
        let ((x0, y0), (x1, y1)) = gradient_line(90.0, 0.0, 0.0, 100.0, 200.0);
        assert!(
            (x0 - 0.0).abs() < 0.01 && (y0 - 100.0).abs() < 0.01,
            "{x0},{y0}"
        );
        assert!(
            (x1 - 100.0).abs() < 0.01 && (y1 - 100.0).abs() < 0.01,
            "{x1},{y1}"
        );
    }

    #[test]
    fn grid_splits_columns_evenly() {
        // Content-sized items sit at track origins (correct CSS behavior:
        // explicit widths don't stretch).
        let cell = |t: &str| Node::text(t, Style::text(32.0, "#fff"));
        let tree = Node::container(
            Style::grid(2).with_size(600.0, 200.0).with_gap(0.0),
            vec![cell("a"), cell("b")],
        );
        let placed = compute_layout(&tree, 600.0, 200.0).unwrap();
        assert_eq!(placed.children.len(), 2);
        let (a, b) = (&placed.children[0], &placed.children[1]);
        assert!((a.x - 0.0).abs() < 1.0, "a.x={}", a.x);
        assert!((b.x - 300.0).abs() < 30.0, "b.x={}", b.x);
        // Auto-sized boxes stretch to fill their tracks.
        let tree = Node::container(
            Style::grid(2).with_size(600.0, 200.0).with_gap(0.0),
            vec![
                Node::container(Style::new().with_background("#111"), vec![]),
                Node::container(Style::new().with_background("#222"), vec![]),
            ],
        );
        let placed = compute_layout(&tree, 600.0, 200.0).unwrap();
        let (a, b) = (&placed.children[0], &placed.children[1]);
        assert!((a.w - 300.0).abs() < 1.0, "a.w={}", a.w);
        assert!((b.w - 300.0).abs() < 1.0, "b.w={}", b.w);
        assert!((b.x - 300.0).abs() < 1.0, "b.x={}", b.x);
    }

    #[test]
    fn text_wraps_at_max_width() {
        let text = "hello world foo bar";
        let wrapped = wrap_text(text, 32.0, 120.0, BUILTIN_FONT);
        assert!(wrapped.contains('\n'), "{wrapped}");
        let style = Style::text(32.0, "#fff").with_max_width(120.0);
        let tree = Node::container(
            Style::column().with_size(600.0, 400.0),
            vec![Node::text(text, style)],
        );
        let placed = compute_layout(&tree, 600.0, 400.0).unwrap();
        let t = &placed.children[0];
        assert!(t.text.as_ref().unwrap().contains('\n'));
        assert!(t.h > 40.0, "h={}", t.h);
    }

    /// 20x10 red PNG bytes for image tests.
    fn test_png() -> Vec<u8> {
        use image::codecs::png::PngEncoder;
        use image::{ExtendedColorType, ImageEncoder, RgbaImage};
        let img = RgbaImage::from_pixel(20, 10, image::Rgba([200, 30, 30, 255]));
        let mut buf = Vec::new();
        PngEncoder::new(&mut buf)
            .write_image(img.as_raw(), 20, 10, ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    #[test]
    fn image_intrinsic_and_aspect() {
        let bytes = test_png();
        assert_eq!(image_dimensions(&bytes).unwrap(), (20, 10));
        // Natural size.
        let tree = Node::image(bytes.clone(), Style::new());
        let placed = compute_layout(&tree, 600.0, 400.0).unwrap();
        assert!((placed.w - 20.0).abs() < 0.5, "w={}", placed.w);
        assert!((placed.h - 10.0).abs() < 0.5, "h={}", placed.h);
        // Width fixed: height back-fills from aspect.
        let tree = Node::image(
            bytes,
            Style {
                width: Some(40.0),
                ..Style::new()
            },
        );
        let placed = compute_layout(&tree, 600.0, 400.0).unwrap();
        assert!((placed.w - 40.0).abs() < 0.5, "w={}", placed.w);
        assert!((placed.h - 20.0).abs() < 1.0, "h={}", placed.h);
    }

    #[test]
    fn absolute_child_offsets() {
        let tree = Node::container(
            Style::column().with_size(600.0, 400.0),
            vec![Node::container(
                Style::new().with_size(50.0, 50.0).absolute_at(10.0, 20.0),
                vec![],
            )],
        );
        let placed = compute_layout(&tree, 600.0, 400.0).unwrap();
        let kid = &placed.children[0];
        assert!((kid.x - 10.0).abs() < 1.0, "x={}", kid.x);
        assert!((kid.y - 20.0).abs() < 1.0, "y={}", kid.y);
    }

    #[test]
    fn hash_covers_new_fields() {
        let a = Node::image(vec![1, 2, 3], Style::grid(2));
        let b = Node::image(vec![1, 2, 4], Style::grid(2));
        assert_ne!(hash_node(&a, 100, 100), hash_node(&b, 100, 100));
    }
}
