//! Sync-prune of stale wheels in `<third_party_dir>/vendor/`.

use std::collections::BTreeSet;
use std::path::Path;

/// Delete `*.whl` entries in `vendor_dir` not in `expected`. If `no_prune`,
/// returns Ok(()) without inspecting the directory. Returns the list of
/// removed filenames (sorted).
pub fn prune_stale(
    _vendor_dir: &Path,
    _expected: &BTreeSet<String>,
    _no_prune: bool,
) -> std::io::Result<Vec<String>> {
    unimplemented!("Task 5")
}
