#![allow(dead_code)] // each test binary uses a subset
pub mod git_fixture;

use assert_cmd::cargo::CommandCargoExt;
use std::process::Command;

pub fn muntjac() -> Command {
    Command::cargo_bin("muntjac").expect("locate muntjac binary")
}
