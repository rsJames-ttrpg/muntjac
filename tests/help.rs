mod common;

use common::muntjac;

#[test]
fn main_help_surface() {
    let output = muntjac().arg("--help").output().expect("run --help");
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    insta::assert_snapshot!("main_help", stdout);
}
