//! Wheel filename parsing and selection.
//!
//! See `docs/superpowers/specs/2026-05-20-muntjac-s2-wheel-selector-design.md`
//! for the design.

pub mod tag;
// `compat` and `select` modules added in later tasks.

pub use tag::{
    AbiTag, LinuxArch, MacArch, PlatformTag, PythonTag, Tag, TagParseError, WheelTag,
};
