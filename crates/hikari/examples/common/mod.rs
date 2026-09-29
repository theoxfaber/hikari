//! Shared output-path helper for the examples.
//!
//! Every example used to hardcode `/Users/apple/hikari/...`, which meant the
//! documented `cargo run --example ...` invocations only worked on the
//! original author's machine and panicked anywhere else. Outputs now land in
//! the workspace root, overridable with `HIKARI_EXAMPLE_OUT`.

use std::path::PathBuf;

/// Directory the examples write their artifacts into.
#[must_use]
pub fn out_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("HIKARI_EXAMPLE_OUT") {
        return PathBuf::from(dir);
    }
    // CARGO_MANIFEST_DIR is <workspace>/crates/hikari. Lexically join and
    // normalize so the printed path is readable rather than full of `../..`.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    dir.canonicalize().unwrap_or(dir)
}

/// Write an example artifact, creating the directory if needed.
pub fn write(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = out_dir();
    std::fs::create_dir_all(&dir).expect("create example output dir");
    let path = dir.join(name);
    std::fs::write(&path, bytes).expect("write example output");
    println!("wrote {} ({} bytes)", path.display(), bytes.len());
    path
}
