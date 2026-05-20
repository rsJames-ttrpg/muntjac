use assert_cmd::cargo::CommandCargoExt;
use std::process::Command;

pub fn muntjac() -> Command {
    Command::cargo_bin("muntjac").expect("locate muntjac binary")
}
