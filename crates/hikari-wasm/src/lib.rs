#![warn(missing_docs)]
//! WebAssembly bindings (free): stills for edge runtimes and browsers.
//!
//! Ships as `hikari_wasm_bg.wasm` + JS glue via `wasm-bindgen --target
//! nodejs` (or `web`). No license keys here: stills are free everywhere.

use hikari::Node;
use wasm_bindgen::prelude::*;

fn parse_tree(json: &str) -> Result<Node, JsValue> {
    serde_json::from_str(json).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Library version.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Render a node-tree JSON document to PNG bytes.
#[wasm_bindgen]
pub fn render_png(tree_json: &str, width: u32, height: u32) -> Result<Vec<u8>, JsValue> {
    let tree = parse_tree(tree_json)?;
    hikari::render_png(&tree, width, height).map_err(|e| JsValue::from_str(&e.to_string()))
}

/// Render a node-tree JSON document to an SVG string.
#[wasm_bindgen]
pub fn render_svg(tree_json: &str, width: u32, height: u32) -> Result<String, JsValue> {
    let tree = parse_tree(tree_json)?;
    hikari::render_svg(&tree, width, height).map_err(|e| JsValue::from_str(&e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_api_renders() {
        let tree = serde_json::to_string(&Node::banner(
            400.0, 200.0, "#0b1020", "Hi Wasm", 48.0, "#ffffff",
        ))
        .unwrap();
        let png = render_png(&tree, 400, 200).unwrap();
        assert_eq!(&png[..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let svg = render_svg(&tree, 400, 200).unwrap();
        assert!(svg.contains("Hi Wasm"));
    }
}
