mod common;

use assert_cmd::prelude::*;
use common::muntjac;
use predicates::str::contains;

#[test]
fn audit_says_unimplemented() {
    muntjac()
        .arg("audit")
        .assert()
        .failure()
        .stderr(contains("not implemented yet (planned for S10)"));
}

#[test]
fn unused_says_unimplemented() {
    muntjac()
        .arg("unused")
        .assert()
        .failure()
        .stderr(contains("not implemented yet (planned for S10)"));
}

#[test]
fn debug_requires_subcommand() {
    muntjac()
        .arg("debug")
        .assert()
        .failure()
        .stderr(contains("debug requires a subcommand"));
}
