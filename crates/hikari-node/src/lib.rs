#![warn(missing_docs)]
//! Node.js bindings.
//!
//! Ships as a single `index.node` built from this crate — no JS toolchain
//! needed to build it, plain `require()` to load it. Every backend is
//! available with no key material, environment variable or feature flag:
//! stills, animation and PDF are all free under the project's MIT/Apache-2.0
//! licence.

use hikari::{Node, PageSize};
use napi::bindgen_prelude::Buffer;
use napi_derive::napi;

fn parse_tree(json: &str) -> napi::Result<Node> {
    serde_json::from_str(json).map_err(|e| napi::Error::from_reason(e.to_string()))
}

fn render_err(e: impl std::fmt::Display) -> napi::Error {
    napi::Error::from_reason(e.to_string())
}

/// Library version.
#[napi]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Register font bytes and return an id to put in a node's `font` field.
///
/// Idempotent on content: registering the same bytes twice returns the existing
/// id, and a server that re-registers its brand font on every request leaks
/// nothing. The `name` is used for PDF and SVG output; lookup is by id.
///
/// The returned id is process-global and valid for the life of the process,
/// so register once at startup rather than per render.
#[napi]
pub fn register_font(name: String, bytes: Buffer) -> napi::Result<u32> {
    hikari::register_font(&name, bytes.as_ref()).map_err(render_err)
}

/// Number of distinct fonts registered, including the embedded one.
#[napi]
pub fn registered_font_count() -> u32 {
    hikari::registered_font_count() as u32
}

/// Render a node-tree JSON document to PNG bytes.
#[napi]
pub fn render_png_sync(tree_json: String, width: u32, height: u32) -> napi::Result<Buffer> {
    let tree = parse_tree(&tree_json)?;
    let bytes = hikari::render_png(&tree, width, height).map_err(render_err)?;
    Ok(Buffer::from(bytes))
}

/// Render a node-tree JSON document to an SVG string.
#[napi]
pub fn render_svg_sync(tree_json: String, width: u32, height: u32) -> napi::Result<String> {
    let tree = parse_tree(&tree_json)?;
    hikari::render_svg(&tree, width, height).map_err(render_err)
}

/// Render a node-tree JSON document to lossless WebP bytes.
#[napi]
pub fn render_webp_sync(tree_json: String, width: u32, height: u32) -> napi::Result<Buffer> {
    let tree = parse_tree(&tree_json)?;
    let placed = hikari::compute_layout(&tree, width as f32, height as f32).map_err(render_err)?;
    let bytes = hikari::render_placed_to_webp(&placed, width, height).map_err(render_err)?;
    Ok(Buffer::from(bytes))
}

/// Render one node-tree JSON document per page to PDF bytes.
#[napi]
pub fn render_pdf_sync(pages_json: String, width: f64, height: f64) -> napi::Result<Buffer> {
    let pages: Vec<Node> =
        serde_json::from_str(&pages_json).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let bytes = hikari::render_pdf(
        &pages,
        PageSize::Custom {
            w: width as f32,
            h: height as f32,
        },
    )
    .map_err(render_err)?;
    Ok(Buffer::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdf_binding_needs_no_key_material() {
        // The old binding read HIKARI_LICENSE and HIKARI_PUBKEY and errored
        // without them. This asserts the absence is now what happens: a clean
        // environment still renders, which is the whole point of dropping the
        // gate. Both variables are cleared first so a stray value in the
        // developer's shell cannot make this pass.
        std::env::remove_var("HIKARI_LICENSE");
        std::env::remove_var("HIKARI_PUBKEY");
        let pages =
            "[{\"Container\":{\"style\":{\"width\":400.0,\"height\":600.0},\"children\":[]}}]";
        let out = render_pdf_sync(pages.to_owned(), 400.0, 600.0).unwrap();
        assert_eq!(&out[..5], b"%PDF-");
    }

    #[test]
    fn png_binding_needs_no_key_material() {
        std::env::remove_var("HIKARI_LICENSE");
        let tree = "{\"Container\":{\"style\":{\"width\":100.0,\"height\":100.0},\"children\":[]}}";
        let out = render_png_sync(tree.to_owned(), 100, 100).unwrap();
        assert_eq!(&out[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
    }
}
