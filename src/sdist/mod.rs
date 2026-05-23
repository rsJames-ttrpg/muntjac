//! Sdist classification and prebake.
//!
//! Distinguishes pure-python sdists (which can be prebaked into wheels via
//! `uv build`) from native sdists (which require platform-specific compile
//! steps muntjac doesn't run in v0.1.0).
//!
//! Public surface used by `cli::vendor` and `buck::emit`.

pub mod classifier;
pub mod error;
pub mod manifest;
pub mod prebake;

pub use classifier::{AllowlistedBackend, Classification, NativeReason, NativeSourceHit, classify};
pub use error::{ClassifyError, SdistError};
pub use manifest::{Manifest, ManifestClassification, ManifestEntry};
pub use prebake::{PrebakeOutput, build_wheel};
