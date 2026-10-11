//! Fuzz target crate.
//!
//! The real targets live in `fuzz_targets/`, compiled by `cargo-fuzz` with
//! sanitizer instrumentation. This library exists so `cargo check` and `cargo
//! deny` have something to resolve; it deliberately contains nothing.

#![no_std]
