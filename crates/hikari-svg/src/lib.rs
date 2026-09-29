#![warn(missing_docs)]
//! SVG backend: `Placed` tree -> resolution-independent SVG string.

use hikari_core::{Color, Placed};

fn hex(c: Color) -> String {
    if c.a == 255 {
        format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
    } else {
        format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Render to an SVG document string.
#[must_use]
pub fn render_to_svg(placed: &Placed, width: u32, height: u32) -> String {
    let mut out = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">"#
    );
    paint(&mut out, placed, 0.0, 0.0);
    out.push_str("</svg>");
    out
}

fn paint(out: &mut String, node: &Placed, dx: f32, dy: f32) {
    use std::fmt::Write as _;
    let x = dx + node.x;
    let y = dy + node.y;
    // `background-clip: text` consumes the background as glyph fill: no box
    // rect here (the text arm below carries the gradient instead).
    let clipped = node.text.is_some() && node.style.clip_text && node.style.background.is_some();
    match (&node.style.shadow, &node.style.background) {
        // Shadow on a background: native filtered drop shadow.
        (Some(shadow), Some(bg)) if !clipped => {
            let id = format!("sh{x:.0}_{y:.0}_{:.0}", shadow.blur);
            let _ = write!(
                out,
                r#"<filter id="{id}" x="-50%" y="-50%" width="200%" height="200%"><feDropShadow dx="{:.1}" dy="{:.1}" stdDeviation="{:.1}" flood-color="{}" flood-opacity="{:.2}"/></filter>"#,
                shadow.dx,
                shadow.dy,
                (shadow.blur / 2.0).max(0.0),
                hex(shadow.color),
                f32::from(shadow.color.a) / 255.0
            );
            paint_background_svg(
                out,
                bg,
                &SvgRect {
                    x,
                    y,
                    w: node.w,
                    h: node.h,
                    radius: node.style.radius,
                    filter: Some(&id),
                },
            );
        }
        // Shadow without a background: sharp offset silhouette (blur needs
        // a source shape; raster paints the geometric shadow regardless).
        (Some(shadow), None) => {
            let _ = write!(
                out,
                r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="{}" opacity="{:.2}"/>"#,
                x + shadow.dx,
                y + shadow.dy,
                node.w,
                node.h,
                node.style.radius,
                hex(shadow.color),
                f32::from(shadow.color.a) / 255.0
            );
        }
        (None, Some(bg)) if !clipped => {
            paint_background_svg(
                out,
                bg,
                &SvgRect {
                    x,
                    y,
                    w: node.w,
                    h: node.h,
                    radius: node.style.radius,
                    filter: None,
                },
            );
        }
        (None, None) => {}
        // Clipped gradients: the text arm below carries the fill.
        _ => {}
    }
    if node.style.border > 0.0 {
        let bc = node
            .style
            .border_color
            .map(hex)
            .unwrap_or_else(|| "#000000".into());
        let _ = write!(
            out,
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" rx="{:.1}" fill="none" stroke="{}" stroke-width="{:.1}"/>"#,
            x, y, node.w, node.h, node.style.radius, bc, node.style.border
        );
    }
    if let Some(text) = &node.text {
        let fs = node.style.font_size.unwrap_or(16.0);
        // `background-clip: text` fills glyphs with the background.
        let fill = match (&node.style.clip_text, &node.style.background) {
            (true, Some(bg)) => bg_fill(out, bg, x, y, node.w, node.h),
            _ => node
                .style
                .color
                .map(hex)
                .unwrap_or_else(|| "#000000".into()),
        };
        let cx = x + node.w / 2.0;
        let cy = y + node.h / 2.0 + fs * 0.35;
        let _ = write!(
            out,
            r#"<text x="{cx:.1}" y="{cy:.1}" font-size="{fs:.1}" fill="{fill}" text-anchor="middle" font-family="DejaVu Sans, sans-serif">{}</text>"#,
            esc(text)
        );
    }
    if let Some(media) = &node.media {
        paint_media(out, media, x, y, node.w, node.h, node.style.radius);
    }
    for child in &node.children {
        paint(out, child, x, y);
    }
}

/// Background rect geometry for SVG.
struct SvgRect<'a> {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    filter: Option<&'a str>,
}

/// Emit a gradient def if `bg` is a gradient; return the fill value
/// (`#hex` for solids, `url(#id)` for gradients).
fn bg_fill(
    out: &mut String,
    bg: &hikari_core::Background,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) -> String {
    use hikari_core::Background;
    use std::fmt::Write as _;
    match bg {
        Background::Solid(c) => hex(*c),
        Background::Linear { angle_deg, stops } => {
            let ((x0, y0), (x1, y1)) = hikari_core::gradient_line(*angle_deg, x, y, w, h);
            let id = format!("lg{x:.0}_{y:.0}_{w:.0}_{h:.0}_{angle_deg:.0}");
            let _ = write!(
                out,
                r#"<linearGradient id="{id}" gradientUnits="userSpaceOnUse" x1="{x0:.1}" y1="{y0:.1}" x2="{x1:.1}" y2="{y1:.1}">"#
            );
            write_stops(out, stops);
            out.push_str("</linearGradient>");
            format!("url(#{id})")
        }
        Background::Radial {
            cx,
            cy,
            radius,
            stops,
        } => {
            let id = format!("rg{x:.0}_{y:.0}_{w:.0}_{h:.0}");
            let rad = if *radius > 0.0 {
                *radius
            } else {
                (w * w + h * h).sqrt() / 2.0
            };
            let _ = write!(
                out,
                r#"<radialGradient id="{id}" gradientUnits="userSpaceOnUse" cx="{:.1}" cy="{:.1}" r="{rad:.1}">"#,
                x + cx * w,
                y + cy * h
            );
            write_stops(out, stops);
            out.push_str("</radialGradient>");
            format!("url(#{id})")
        }
    }
}

fn write_stops(out: &mut String, stops: &[hikari_core::ColorStop]) {
    use std::fmt::Write as _;
    for s in stops {
        let _ = write!(
            out,
            r#"<stop offset="{:.3}" stop-color="{}" stop-opacity="{:.3}"/>"#,
            s.pos.clamp(0.0, 1.0),
            hex(s.color),
            f32::from(s.color.a) / 255.0
        );
    }
}

/// Paint any background fill for SVG, with gradient defs as needed.
fn paint_background_svg(out: &mut String, bg: &hikari_core::Background, r: &SvgRect<'_>) {
    use std::fmt::Write as _;
    let (x, y, w, h, radius) = (r.x, r.y, r.w, r.h, r.radius);
    let fattr = r
        .filter
        .map(|id| format!(r#" filter="url(#{id})""#))
        .unwrap_or_default();
    let fill = bg_fill(out, bg, x, y, w, h);
    let _ = write!(
        out,
        r#"<rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" rx="{radius:.1}" fill="{fill}"{fattr}/>"#
    );
}

/// Embed an image leaf as a base64 data URI with rounded clipping.
fn paint_media(
    out: &mut String,
    media: &hikari_core::Media,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
) {
    use std::fmt::Write as _;
    let hikari_core::Media::Image { bytes, .. } = media;
    let mime = if bytes.len() >= 3 && bytes[0] == 0xFF && bytes[1] == 0xD8 {
        "image/jpeg"
    } else {
        "image/png"
    };
    let data = base64::engine::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes);
    let clip = if radius > 0.5 {
        let id = format!("c{:.0}{:.0}{:.0}{:.0}", x, y, w, h);
        let _ = write!(
            out,
            r#"<clipPath id="{id}"><rect x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}" rx="{radius:.1}"/></clipPath>"#
        );
        format!(r#" clip-path="url(#{id})""#)
    } else {
        String::new()
    };
    let _ = write!(
        out,
        r#"<image x="{x:.1}" y="{y:.1}" width="{w:.1}" height="{h:.1}"{clip} href="data:{mime};base64,{data}"/>"#
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use hikari_core::{compute_layout, Node};

    #[test]
    fn svg_contains_shapes_and_text() {
        let tree = Node::banner(1200.0, 630.0, "#0b1020", "Hello", 72.0, "#ffffff");
        let placed = compute_layout(&tree, 1200.0, 630.0).unwrap();
        let svg = render_to_svg(&placed, 1200, 630);
        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("Hello"));
        assert!(svg.contains("#0b1020"));
    }

    #[test]
    fn svg_drop_shadow_filter() {
        let tree = Node::container(
            hikari_core::Style::new()
                .with_size(200.0, 100.0)
                .with_background("#ffffff")
                .with_shadow(4.0, 6.0, 10.0, 0.0, "#000000"),
            vec![],
        );
        let placed = compute_layout(&tree, 400.0, 300.0).unwrap();
        let svg = render_to_svg(&placed, 400, 300);
        assert!(svg.contains("feDropShadow"), "{svg}");
        assert!(svg.contains("flood-color"));
    }

    #[test]
    fn svg_clip_text_uses_gradient_fill() {
        let tree = Node::container(
            hikari_core::Style::new().with_size(400.0, 200.0),
            vec![Node::text(
                "Hi",
                hikari_core::Style::text(64.0, "#ff0000")
                    .clip_text()
                    .with_linear_gradient(90.0, &[(0.0, "#000000"), (1.0, "#ffffff")]),
            )],
        );
        let placed = compute_layout(&tree, 400.0, 200.0).unwrap();
        let svg = render_to_svg(&placed, 400, 200);
        assert!(svg.contains("<linearGradient"), "{svg}");
        assert!(svg.contains("fill=\"url(#"), "{svg}");
    }

    #[test]
    fn svg_embeds_gradients() {
        let tree = Node::container(
            hikari_core::Style::new()
                .with_size(400.0, 200.0)
                .with_linear_gradient(180.0, &[(0.0, "#dbeafe"), (1.0, "#fee2e2")]),
            vec![],
        );
        let placed = compute_layout(&tree, 400.0, 200.0).unwrap();
        let svg = render_to_svg(&placed, 400, 200);
        assert!(svg.contains("<linearGradient"));
        assert!(svg.contains("#dbeafe") && svg.contains("#fee2e2"));
    }
}
