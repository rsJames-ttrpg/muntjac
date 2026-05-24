//! Local & community fixup configuration parsing, layering, and resolution.

pub mod cfg;
pub mod error;
pub mod layer;
pub mod loader;
pub mod overlay;
pub mod registry;
pub mod schema;
pub mod validate;

pub use cfg::{CfgContext, CfgPredicate, split_target_triple};
pub use error::{CfgParseError, FixupError};
pub use layer::{ResolvedFixup, resolve_for_cell};
pub use loader::{FixupSet, load_community, load_local};
pub use overlay::discover_overlay_files;
pub use registry::{RegistryConfig, parse_registry_config};
pub use schema::{EntryPoints, FixupBody, FixupConfig, SdistFixup};
pub use validate::is_valid_buck_target;

#[cfg(test)]
mod smoke_tests {
    #[test]
    fn module_compiles() {
        // Pure compile-time gate: ensures every re-exported item resolves.
    }
}
