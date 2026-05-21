//! Hand-rolled string-writer implementation of `BuckEmitter`.

use super::emit::{BuckEmitter, EmitInput, EmitOutput};

pub struct StringTemplateEmitter;

impl BuckEmitter for StringTemplateEmitter {
    fn emit(&self, _input: &EmitInput) -> EmitOutput {
        EmitOutput {
            buck: String::new(),
            muntjac_bzl: String::new(),
            config_buck: String::new(),
            package_file: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buck::emit::ConfigName;

    fn empty_input() -> EmitInput {
        EmitInput {
            tree: "default".into(),
            third_party_dir: "third-party/python".into(),
            configs: vec![ConfigName::new("3.12", "linux-x86_64-gnu")],
            packages: vec![],
        }
    }

    #[test]
    fn emitter_returns_four_strings() {
        let out = StringTemplateEmitter.emit(&empty_input());
        let _ = out.buck;
        let _ = out.muntjac_bzl;
        let _ = out.config_buck;
        let _ = out.package_file;
    }
}
