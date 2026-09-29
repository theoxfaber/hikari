#![warn(missing_docs)]
//! Node.js bindings (free): stills everywhere JS runs, Pro PDF behind keys.
//!
//! Ships as a single `index.node` built from this crate — no JS toolchain
//! needed to build it, plain `require()` to load it. PDF verification uses
//! operator-provided keys: `HIKARI_LICENSE` (the `hk1.…` key) and
//! `HIKARI_PUBKEY` (hex of the 32-byte ed25519 verify key from your signing
//! service). Unset key material means unlicensed: stills work, PDF errors.

use hikari::{License, Node, PageSize};
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

/// License gate for Pro calls: key + pubkey both come from the environment
/// so operators keep their own signing infrastructure.
fn pro_license() -> napi::Result<License> {
    let key = std::env::var("HIKARI_LICENSE").map_err(|_| {
        napi::Error::from_reason(
            "HIKARI_LICENSE is not set (Pro PDF needs a license key)".to_owned(),
        )
    })?;
    let pub_hex = std::env::var("HIKARI_PUBKEY")
        .map_err(|_| napi::Error::from_reason("HIKARI_PUBKEY is not set".to_owned()))?;
    let pub_bytes = hex_decode(&pub_hex)
        .map_err(|_| napi::Error::from_reason("HIKARI_PUBKEY must be 64 hex chars".to_owned()))?;
    if pub_bytes.len() != 32 {
        return Err(napi::Error::from_reason(
            "HIKARI_PUBKEY must be 64 hex chars".to_owned(),
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&pub_bytes);
    License::verify(&key, &arr, hikari::now_unix())
        .map_err(|e| napi::Error::from_reason(format!("license rejected: {e}")))
}

fn hex_decode(s: &str) -> Result<Vec<u8>, ()> {
    if !s.len().is_multiple_of(2) {
        return Err(());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| ()))
        .collect()
}

/// Render one node-tree JSON document per page to PDF bytes (Pro).
/// Reads `HIKARI_LICENSE` / `HIKARI_PUBKEY` from the environment.
#[napi]
pub fn render_pdf_sync(pages_json: String, width: f64, height: f64) -> napi::Result<Buffer> {
    let license = pro_license()?;
    let pages: Vec<Node> =
        serde_json::from_str(&pages_json).map_err(|e| napi::Error::from_reason(e.to_string()))?;
    let bytes = hikari::render_pdf(
        &pages,
        PageSize::Custom {
            w: width as f32,
            h: height as f32,
        },
        &license,
    )
    .map_err(render_err)?;
    Ok(Buffer::from(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SK: [u8; 32] = [11u8; 32];

    fn env_with_pro_key() -> (String, String) {
        use ed25519_dalek::SigningKey;
        let sk = SigningKey::from_bytes(&SK);
        let pk = sk.verifying_key().to_bytes();
        let key = License::mint(hikari::Plan::Pro, 0, "node-test", &SK);
        (key, pk.iter().map(|b| format!("{b:02x}")).collect())
    }

    #[test]
    fn pdf_binding_honors_env_keys() {
        let (key, pubkey) = env_with_pro_key();
        std::env::set_var("HIKARI_LICENSE", &key);
        std::env::set_var("HIKARI_PUBKEY", &pubkey);
        let pages =
            "[{\"Container\":{\"style\":{\"width\":400.0,\"height\":600.0},\"children\":[]}}]";
        let out = render_pdf_sync(pages.to_owned(), 400.0, 600.0).unwrap();
        assert_eq!(&out[..5], b"%PDF-");
    }

    #[test]
    fn pdf_binding_errors_without_keys() {
        std::env::remove_var("HIKARI_LICENSE");
        std::env::remove_var("HIKARI_PUBKEY");
        match render_pdf_sync("[]".to_owned(), 100.0, 100.0) {
            Err(e) => assert!(e.reason.contains("HIKARI_LICENSE"), "{}", e.reason),
            Ok(_) => panic!("expected license error"),
        }
    }
}
