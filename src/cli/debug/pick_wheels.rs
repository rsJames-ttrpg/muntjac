//! `muntjac debug pick-wheels` — print the wheel selected for each
//! (package, version, platform, python_version) cell as JSON.

use anyhow::Result;

use crate::cli::Globals;
use crate::cli::debug::PickWheelsArgs;

pub fn run(_args: PickWheelsArgs, _globals: &Globals) -> Result<()> {
    // Stub: actual logic lands in Task 16.
    unimplemented!("pick-wheels stub")
}
