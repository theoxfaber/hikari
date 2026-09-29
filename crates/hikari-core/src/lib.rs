#![warn(missing_docs)]
//! `hikari-core`: node tree, styles, `taffy` layout, content-hash cache.
//!
//! Intentionally small: containers + text only. This covers most OG cards
//! while keeping layout predictable and the dependency graph upstream-only.

mod cache;
mod error;
mod flow;
mod shape;

pub use cache::{hash_bytes, hash_node, HashCache};
pub use error::Error;
pub use flow::{paginate, Flow};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Border radius in px.
    #[serde(default)]
    pub radius: f32,
    /// Flex grow factor.
    #[serde(default)]
    pub grow: f32,
    /// Box shadow (painted beneath border/background).
    #[serde(default)]
    pub shadow: Option<Shadow>,
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
            font_size: None,
            radius: 0.0,
            grow: 0.0,
            bookmark: None,
            keep_together: false,
            repeat_header: false,
            shadow: None,
            link: None,
            clip_text: false,
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
            dx,
            dy,
            blur: blur.max(0.0),
            spread,
            color: Color::from_hex(hex),
        });
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
            let laid = match self.max_width {
                Some(mw) => wrap_text(t, fs, mw),
                None => t.to_owned(),
            };
            let (mw, mh) = measure_text(&laid, fs);
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
        bytes: Vec<u8>,
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

    /// Image constructor (`fit` defaults to cover).
    #[must_use]
    pub fn image(bytes: Vec<u8>, style: Style) -> Self {
        Self::Image {
            bytes,
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

/// Decode header (full decode for now) to get intrinsic image dimensions.
pub fn image_dimensions(bytes: &[u8]) -> Result<(u32, u32), Error> {
    let img = image::load_from_memory(bytes).map_err(|e| Error::Asset(e.to_string()))?;
    Ok((img.width(), img.height()))
}

/// Compute flex layout for `tree` inside `viewport_w` x `viewport_h`.
pub fn compute_layout(tree: &Node, viewport_w: f32, viewport_h: f32) -> Result<Placed, Error> {
    let mut taffy: TaffyTree<()> = TaffyTree::new();
    let root_id = build_taffy(tree, &mut taffy)?;
    taffy.compute_layout(
        root_id,
        Size {
            width: AvailableSpace::Definite(viewport_w),
            height: AvailableSpace::Definite(viewport_h),
        },
    )?;
    Ok(read_back(tree, &taffy, root_id))
}

fn build_taffy(node: &Node, taffy: &mut TaffyTree<()>) -> Result<taffy::NodeId, Error> {
    match node {
        Node::Container { style, children } => {
            let ids = children
                .iter()
                .map(|c| build_taffy(c, taffy))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(taffy.new_with_children(style.to_taffy(None, None), &ids)?)
        }
        Node::Text { text, style } => Ok(taffy.new_leaf(style.to_taffy(Some(text), None))?),
        Node::Image { bytes, style, .. } => {
            let dims = image_dimensions(bytes).ok();
            Ok(taffy.new_leaf(style.to_taffy(None, dims))?)
        }
    }
}

fn read_back(node: &Node, taffy: &TaffyTree<()>, id: taffy::NodeId) -> Placed {
    let layout = taffy.layout(id).expect("computed");
    let (style, text, link, media, kids) = match node {
        Node::Container { style, children } => {
            let child_ids = taffy.children(id).expect("children");
            let placed = children
                .iter()
                .zip(child_ids)
                .map(|(c, cid)| read_back(c, taffy, cid))
                .collect();
            (style.clone(), None, style.link.clone(), None, placed)
        }
        Node::Text { text, style } => {
            // Store the wrapped text so paint wraps identically to layout.
            let laid = match style.max_width {
                Some(mw) => wrap_text(text, style.font_size.unwrap_or(16.0), mw),
                None => text.clone(),
            };
            (
                style.clone(),
                Some(laid),
                style.link.clone(),
                None,
                Vec::new(),
            )
        }
        Node::Image { bytes, fit, style } => (
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
    Placed {
        x: layout.location.x,
        y: layout.location.y,
        w: layout.size.width,
        h: layout.size.height,
        style,
        text,
        link,
        media,
        children: kids,
    }
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
        let wrapped = wrap_text(text, 32.0, 120.0);
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
