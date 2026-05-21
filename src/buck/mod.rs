//! Buck2 BUCK + muntjac.bzl + config/BUCK + PACKAGE emitter.
//!
//! See `docs/superpowers/specs/2026-05-21-muntjac-s3-buck-emitter-design.md`
//! for the design.

pub mod emit;

pub use emit::{BuckEmitter, ConfigName, EmitInput, EmitOutput, EmitPackage, EmitWheel};
