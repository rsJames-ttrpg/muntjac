use anyhow::Result;

pub fn run(verb: &str, planned_for: &str) -> Result<()> {
    anyhow::bail!(
        "muntjac {verb} is not implemented yet (planned for {planned_for}); \
         see docs/superpowers/specs/2026-05-20-muntjac-roadmap.md"
    )
}
