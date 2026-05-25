//! Buck2 BUCK + muntjac.bzl + config/BUCK + wiring.bzl emitter.
//!
//! See `docs/superpowers/specs/2026-05-21-muntjac-s3-buck-emitter-design.md`
//! for the design.

pub mod emit;
pub mod string_writer;
pub mod write;

pub use emit::{
    BuckEmitter, BuildEmitContext, ConfigName, EmitInput, EmitOutput, EmitPackage, EmitWheel,
    SharedCfgInput, SharedCfgOutput, build_emit_input, build_shared_cfg_input,
};
pub use string_writer::{StringTemplateEmitter, emit_shared_cfg};
pub use write::{write_outputs, write_shared_cfg};
