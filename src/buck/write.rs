//! Filesystem writer for EmitOutput.

use std::path::Path;

use super::emit::EmitOutput;

pub fn write_outputs(output: &EmitOutput, third_party_dir: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    std::fs::create_dir_all(third_party_dir)
        .with_context(|| format!("creating {}", third_party_dir.display()))?;
    let config_dir = third_party_dir.join("config");
    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("creating {}", config_dir.display()))?;

    atomic_write(&third_party_dir.join("BUCK"), &output.buck)?;
    atomic_write(&third_party_dir.join("muntjac.bzl"), &output.muntjac_bzl)?;
    atomic_write(&third_party_dir.join("wiring.bzl"), &output.wiring_bzl)?;
    atomic_write(&config_dir.join("BUCK"), &output.config_buck)?;
    Ok(())
}

fn atomic_write(path: &Path, contents: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_outputs_creates_all_four_files() {
        let tmp = tempfile::tempdir().unwrap();
        let tpd = tmp.path().join("third-party/python");

        let out = EmitOutput {
            buck: "BUCK_BODY\n".into(),
            muntjac_bzl: "BZL_BODY\n".into(),
            config_buck: "CONFIG_BODY\n".into(),
            wiring_bzl: "WIRING_BODY\n".into(),
        };

        write_outputs(&out, &tpd).unwrap();

        assert_eq!(
            std::fs::read_to_string(tpd.join("BUCK")).unwrap(),
            "BUCK_BODY\n"
        );
        assert_eq!(
            std::fs::read_to_string(tpd.join("muntjac.bzl")).unwrap(),
            "BZL_BODY\n"
        );
        assert_eq!(
            std::fs::read_to_string(tpd.join("wiring.bzl")).unwrap(),
            "WIRING_BODY\n"
        );
        assert_eq!(
            std::fs::read_to_string(tpd.join("config/BUCK")).unwrap(),
            "CONFIG_BODY\n"
        );
        for entry in std::fs::read_dir(&tpd).unwrap() {
            let path = entry.unwrap().path();
            assert!(
                path.extension().is_none_or(|e| e != "tmp"),
                "leftover .tmp file: {:?}",
                path
            );
        }
    }

    #[test]
    fn write_outputs_overwrites_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let tpd = tmp.path().join("third-party/python");

        let out1 = EmitOutput {
            buck: "OLD\n".into(),
            muntjac_bzl: String::new(),
            config_buck: String::new(),
            wiring_bzl: String::new(),
        };
        write_outputs(&out1, &tpd).unwrap();
        assert_eq!(std::fs::read_to_string(tpd.join("BUCK")).unwrap(), "OLD\n");

        let out2 = EmitOutput {
            buck: "NEW\n".into(),
            muntjac_bzl: String::new(),
            config_buck: String::new(),
            wiring_bzl: String::new(),
        };
        write_outputs(&out2, &tpd).unwrap();
        assert_eq!(std::fs::read_to_string(tpd.join("BUCK")).unwrap(), "NEW\n");
    }
}
