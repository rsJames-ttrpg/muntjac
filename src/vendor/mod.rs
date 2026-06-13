//! Vendor mode — download and commit wheels into `<third_party_dir>/vendor/`.
//!
//! See `docs/superpowers/specs/2026-05-26-muntjac-s9-vendor-mode-design.md`.

pub mod download;
pub mod sync;

pub use download::download_wheel;
pub use sync::prune_stale;
