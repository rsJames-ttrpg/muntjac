//! Local & community fixup configuration parsing, layering, and resolution.

pub mod cfg;
pub mod error;
pub mod layer;
pub mod loader;
pub mod schema;

pub use cfg::{CfgContext, CfgPredicate, split_target_triple};
pub use error::{CfgParseError, FixupError};
pub use layer::{ResolvedFixup, resolve_for_cell};
pub use loader::{FixupSet, load_local};
pub use schema::{EntryPoints, FixupBody, FixupConfig, SdistFixup};

#[cfg(test)]
mod smoke_tests {
    #[test]
    fn module_compiles() {
        // Pure compile-time gate: ensures every re-exported item resolves.
    }
}
