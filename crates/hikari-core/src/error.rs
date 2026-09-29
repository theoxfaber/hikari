//! Error type shared by all `hikari` crates.

use thiserror::Error;

/// All errors produced by `hikari`.
#[derive(Debug, Error)]
pub enum Error {
    /// Layout failed.
    #[error("layout error: {0}")]
    Layout(#[from] taffy::TaffyError),
    /// Raster backend failed.
    #[error("raster error: {0}")]
    Raster(String),
    /// Font load failed.
    #[error("font error: {0}")]
    Font(String),
    /// Encoding failed.
    #[error("encode error: {0}")]
    Encode(String),
    /// Asset (image/font) load failed.
    #[error("asset error: {0}")]
    Asset(String),
    /// License check failed (Pro feature without entitlement).
    #[error("license error: {0}")]
    License(String),
}
