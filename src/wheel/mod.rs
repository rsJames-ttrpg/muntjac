//! Wheel filename parsing and selection.
//!
//! See `docs/superpowers/specs/2026-05-20-muntjac-s2-wheel-selector-design.md`
//! for the design.

pub mod compat;
pub mod select;
pub mod tag;

pub use compat::{CompatibleTags, build_compatible_tags};
pub use select::{PickResult, pick_wheel};
pub use tag::{
    AbiTag, LinuxArch, MacArch, PlatformTag, PythonTag, Tag, TagParseError, WheelTag,
    parse_filename,
};
