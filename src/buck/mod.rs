//! Buck2 BUCK + muntjac.bzl + config/BUCK + PACKAGE emitter.
//!
//! See `docs/superpowers/specs/2026-05-21-muntjac-s3-buck-emitter-design.md`
//! for the design.

pub mod emit;
pub mod string_writer;

pub use emit::{
    BuckEmitter, ConfigName, EmitInput, EmitOutput, EmitPackage, EmitWheel, build_emit_input,
};
pub use string_writer::StringTemplateEmitter;
