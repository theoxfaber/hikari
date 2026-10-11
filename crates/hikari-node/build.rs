//! Link the N-API symbols correctly on macOS.
//!
//! # Why this exists
//!
//! `hikari-rs-node` is a `cdylib` whose undefined symbols are the N-API
//! functions, which are supplied by the Node process that loads it at runtime
//! rather than by any library on the system. The Linux and Windows linkers
//! tolerate that by default; Apple's does not.
//!
//! `-undefined dynamic_lookup` tells the macOS linker to leave unresolved
//! symbols in the image for the dynamic loader to bind later, which is exactly
//! the model N-API uses. Without it, building the binding on a Mac fails with a
//! wall of `symbol(s) not found for architecture arm64` -- every N-API function
//! reported undefined.
//!
//! It went unnoticed until the Node matrix was widened from Linux to macOS,
//! because the crate had only ever been linked on one platform. The error reads
//! like a missing dependency rather than a missing linker flag.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "macos" {
        // Passed as two separate `-C link-arg`s rather than one string: rustc
        // does not re-split a link-arg containing spaces, and the silent result
        // is a build that links nothing differently and still fails.
        println!("cargo:rustc-cdylib-link-arg=-undefined");
        println!("cargo:rustc-cdylib-link-arg=dynamic_lookup");
    }
}
